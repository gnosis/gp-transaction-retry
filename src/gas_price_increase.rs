//! Transactions with the same nonce must have a minimum gas price increase.

use futures::stream::{Stream, StreamExt as _};
use gas_estimation::EstimatedGasPrice;

/// openethereum requires that the gas price of the resubmitted transaction has increased by at
/// least 12.5%.
const MIN_GAS_PRICE_INCREASE_FACTOR: f64 = 1.125 * (1.0 + f64::EPSILON);

/// The minimum gas price that allows a new transaction to replace an older one.
pub fn minimum_increase(previous_gas_price: EstimatedGasPrice) -> EstimatedGasPrice {
    previous_gas_price
        .bump_cap(MIN_GAS_PRICE_INCREASE_FACTOR)
        .ceil_cap()
}

fn new_gas_price_estimate(
    previous_gas_price: EstimatedGasPrice,
    new_gas_price: EstimatedGasPrice,
    max_gas_price: f64,
) -> Option<EstimatedGasPrice> {
    let min_gas_price = minimum_increase(previous_gas_price);
    if min_gas_price.cap() > max_gas_price {
        return None;
    }
    if new_gas_price.cap() <= previous_gas_price.cap() {
        // Gas price has not increased.
        return None;
    }
    // Gas price could have increased but doesn't respect minimum increase so adjust it up.
    let new_price = if min_gas_price.cap() >= new_gas_price.cap() {
        min_gas_price
    } else {
        new_gas_price
    };

    Some(new_price.limit_cap(max_gas_price))
}

/// Adapt a stream of gas prices to only yield gas prices that respect the minimum gas price
/// increase while filtering out other values, including those over the cap.
/// Panics if gas price is negative or not finite.
pub fn enforce_minimum_increase_and_cap(
    gas_price_cap: f64,
    stream: impl Stream<Item = EstimatedGasPrice>,
) -> impl Stream<Item = EstimatedGasPrice> {
    let mut last_used_gas_price = Default::default();
    stream.filter_map(move |gas_price| {
        assert!(gas_price.cap().is_finite() && gas_price.cap() >= 0.0);
        let gas_price = if let Some(previous) = last_used_gas_price {
            new_gas_price_estimate(previous, gas_price, gas_price_cap)
        } else {
            Some(gas_price.limit_cap(gas_price_cap))
        };
        if let Some(gas_price) = gas_price {
            last_used_gas_price = Some(gas_price);
        }
        async move { gas_price }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    use gas_estimation::EstimatedGasPrice;

    fn legacy_gas_price(legacy: f64) -> EstimatedGasPrice {
        EstimatedGasPrice {
            legacy,
            ..Default::default()
        }
    }

    #[test]
    fn new_gas_price_estimate_legacy() {
        // new below previous
        assert_eq!(
            new_gas_price_estimate(legacy_gas_price(10.0), legacy_gas_price(0.0), 20.0),
            None
        );
        //new equal to previous
        assert_eq!(
            new_gas_price_estimate(legacy_gas_price(10.0), legacy_gas_price(10.0), 20.0),
            None
        );
        // between previous and min increase rounded up to min increase
        assert_eq!(
            new_gas_price_estimate(legacy_gas_price(10.0), legacy_gas_price(11.0), 20.0),
            Some(legacy_gas_price(12.0))
        );
        // between min increase and max stays same
        assert_eq!(
            new_gas_price_estimate(legacy_gas_price(10.0), legacy_gas_price(13.0), 20.0),
            Some(legacy_gas_price(13.0))
        );
        // larger than max stays max
        assert_eq!(
            new_gas_price_estimate(legacy_gas_price(10.0), legacy_gas_price(20.0), 20.0),
            Some(legacy_gas_price(20.0))
        );
        // cannot increase by min increase
        assert_eq!(
            new_gas_price_estimate(legacy_gas_price(19.0), legacy_gas_price(18.0), 20.0),
            None
        );
        assert_eq!(
            new_gas_price_estimate(legacy_gas_price(19.0), legacy_gas_price(19.0), 20.0),
            None
        );
        assert_eq!(
            new_gas_price_estimate(legacy_gas_price(19.0), legacy_gas_price(19.5), 20.0),
            None
        );
        assert_eq!(
            new_gas_price_estimate(legacy_gas_price(19.0), legacy_gas_price(20.0), 20.0),
            None
        );
        assert_eq!(
            new_gas_price_estimate(legacy_gas_price(19.0), legacy_gas_price(25.0), 20.0),
            None
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn stream_enforces_minimum_increase_legacy() {
        let input_stream = futures::stream::iter(vec![
            legacy_gas_price(0.0),
            legacy_gas_price(1.0),
            legacy_gas_price(1.0),
            legacy_gas_price(2.0),
            legacy_gas_price(2.5),
            legacy_gas_price(0.5),
        ]);
        let stream = enforce_minimum_increase_and_cap(2.0, input_stream);
        let result = stream.collect::<Vec<_>>().now_or_never().unwrap();
        assert_eq!(
            result,
            &[
                legacy_gas_price(0.0),
                legacy_gas_price(1.0),
                legacy_gas_price(2.0)
            ]
        );
    }

    #[test]
    #[allow(clippy::float_cmp)]
    fn stream_enforces_cap_on_first_item_legacy() {
        let input_stream = futures::stream::iter(vec![legacy_gas_price(1500.0)]);
        let stream = enforce_minimum_increase_and_cap(500.0, input_stream);
        let result = stream.collect::<Vec<_>>().now_or_never().unwrap();
        assert_eq!(result, &[legacy_gas_price(500.0)]);
    }
}

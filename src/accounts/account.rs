use std::collections::HashMap;

use crate::{
    positions::position::MicroEnginePosition,
    round_float_to_digits,
    settings::{MicroEngineTradingGroupSettings, TradingGroupInstrumentSettings},
};

#[derive(Debug, Clone)]
pub struct MicroEngineAccountCalculationUpdate {
    pub account_id: String,
    pub margin: f64,
    pub equity: f64,
    pub free_margin: f64,
    pub margin_level: f64,
    pub total_gross: f64,
    pub balance: f64,
}

#[derive(Debug, Clone)]
pub struct MicroEngineAccount {
    pub id: String,
    pub trader_id: String,
    pub trading_group: String,
    pub balance: f64,
    pub leverage: f64,
    pub margin: f64,
    pub equity: f64,
    pub free_margin: f64,
    pub margin_level: f64,
}

impl MicroEngineAccount {
    pub fn recalculate_account_data(
        &mut self,
        account_positions: &[&MicroEnginePosition],
        settings: &MicroEngineTradingGroupSettings,
    ) -> MicroEngineAccountCalculationUpdate {
        let (raw_margin, gross_pl) =
            self.calculate_margin_and_gross_pl(account_positions, settings.hedge_coef, settings);

        // The trading engine is authoritative for these numbers (it triggers margin calls and
        // stop-outs), so they are rounded exactly as trading-engine-core does
        // (positions/cache/account_statistic.rs):
        // - the account margin is rounded to the collateral's digits,
        // - free margin is derived from the ROUNDED margin,
        // - margin level is derived from the rounded margin and rounded to 2 decimals,
        // - margin level is 0 for an account without positions.
        let margin = round_float_to_digits(
            raw_margin,
            account_collateral_digits(account_positions, settings),
        );

        self.margin = margin;
        self.equity = self.balance + gross_pl;
        self.free_margin = self.equity - self.margin;
        self.margin_level = calculate_margin_level(self.equity, margin, account_positions.is_empty());

        MicroEngineAccountCalculationUpdate {
            account_id: self.id.clone(),
            margin: self.margin,
            equity: self.equity,
            free_margin: self.free_margin,
            margin_level: self.margin_level,
            total_gross: gross_pl,
            balance: self.balance,
        }
    }

    fn calculate_margin_and_gross_pl(
        &self,
        account_positions: &[&MicroEnginePosition],
        hedge_coef: Option<f64>,
        settings: &MicroEngineTradingGroupSettings,
    ) -> (f64, f64) {
        let mut total_margin = 0.0;
        let mut total_gross_pl = 0.0;
        let mut grouped_positions = HashMap::new();

        for position in account_positions.into_iter() {
            grouped_positions
                .entry(position.asset_pair.clone())
                .or_insert_with(Vec::new)
                .push(*position);
        }

        for (asset, positions) in grouped_positions.into_iter() {
            if let Some(target_settings) = settings.instruments.get(&asset) {
                let (margin, gross) = calculate_specific_instrument_margin_and_gross_pl(
                    positions.as_slice(),
                    self,
                    hedge_coef,
                    target_settings,
                );
                total_margin += margin;
                total_gross_pl += gross;
            }
        }

        (total_margin, total_gross_pl)
    }
}

/// Digits of the account's collateral. The account margin is non-zero only when the account has
/// positions, and every position is held in the account's collateral, so the positions name it.
/// Falls back to 2, like the position P&L rounding, when the collateral has no settings.
pub(crate) fn account_collateral_digits(
    account_positions: &[&MicroEnginePosition],
    settings: &MicroEngineTradingGroupSettings,
) -> i32 {
    account_positions
        .iter()
        .find_map(|position| settings.collaterals.get(&position.collateral))
        .map(|collateral| collateral.digits as i32)
        .unwrap_or(2)
}

/// `equity / margin * 100`, rounded to 2 decimals, from the already rounded margin; 0 without
/// positions. With positions and a margin that rounds to 0 the engine would divide by zero; this
/// returns 0 there instead of a non-finite number, which JSON cannot carry.
pub(crate) fn calculate_margin_level(equity: f64, rounded_margin: f64, has_no_positions: bool) -> f64 {
    if has_no_positions || rounded_margin == 0.0 {
        return 0.0;
    }

    round_float_to_digits(equity / rounded_margin * 100.0, 2)
}

fn calculate_specific_instrument_margin_and_gross_pl(
    positions: &[&MicroEnginePosition],
    account: &MicroEngineAccount,
    hedge_coef: Option<f64>,
    settings: &TradingGroupInstrumentSettings,
) -> (f64, f64) {
    if positions.is_empty() {
        return (0.0, 0.0);
    }

    let mut total_gross_pl = 0.0;

    let leverage = match settings.max_leverage {
        Some(x) => x.min(account.leverage),
        None => account.leverage,
    };

    let mut buy_margin_price_sum = 0.0;
    let mut sell_margin_price_sum = 0.0;

    let mut buy_volume = 0.0;
    let mut sell_volume = 0.0;
    let mut contract_size_sum = 0.0;

    for position in positions {
        total_gross_pl += position.get_gross_pl();
        let margin_price = position.margin_bidask.get_open_price(position.is_buy);
        match position.is_buy {
            true => {
                buy_margin_price_sum += margin_price * position.lots_amount;
                buy_volume += position.lots_amount;
                contract_size_sum += position.contract_size;
            }
            false => {
                sell_margin_price_sum += margin_price * position.lots_amount;
                sell_volume += position.lots_amount;
                contract_size_sum += position.contract_size;
            }
        }
    }

    let positions_len = positions.len() as f64;
    let contract_size = contract_size_sum / positions_len;
    let hedged_volume = buy_volume.min(sell_volume);

    let hedged_margin = {
        if buy_volume > 0.0 && sell_volume > 0.0 {
            let hedged_margin_coef = hedge_coef.unwrap_or(1.0);

            let hedged_margin_price =
                (buy_margin_price_sum + sell_margin_price_sum) / (buy_volume + sell_volume);

            hedged_volume * contract_size * hedged_margin_price / leverage * hedged_margin_coef
        } else {
            0.0
        }
    };

    let not_hedged_margin_price = match buy_volume > sell_volume {
        true => buy_margin_price_sum / buy_volume,
        false => sell_margin_price_sum / sell_volume,
    };

    let not_hedged_volume = (buy_volume - sell_volume).abs();

    let not_hedge_margin = not_hedged_volume * contract_size * not_hedged_margin_price / leverage;
    (hedged_margin + not_hedge_margin, total_gross_pl)
}

#[cfg(test)]
mod test {
    use crate::{
        accounts::account::{
            MicroEngineAccount, calculate_specific_instrument_margin_and_gross_pl,
        },
        bidask::dto::MicroEngineBidask,
        positions::position::MicroEnginePosition,
        settings::{TradingGroupInstrumentMarkupSettings, TradingGroupInstrumentSettings},
    };

    #[tokio::test]
    pub async fn test_account_margin_calculation() {
        let position = vec![MicroEnginePosition {
            id: "id".to_string(),
            trader_id: "TR1".to_string(),
            account_id: "ACC1".to_string(),
            base: "EUR".to_string(),
            quote: "USD".to_string(),
            collateral: "USD".to_string(),
            asset_pair: "EURUSD".to_string(),
            lots_amount: 0.05,
            contract_size: 100000.0,
            is_buy: false,
            pl: 0.0,
            commission: 0.00,
            open_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            active_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            margin_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            profit_bidask: MicroEngineBidask::create_blank(),
            profit_price_assets_subscriptions: vec![],
            swaps_sum: 0.0,
            ..Default::default()
        }];

        let account = MicroEngineAccount {
            id: "ACC1".to_string(),
            trader_id: "TR1".to_string(),
            trading_group: "tg1".to_string(),
            balance: 99488.14,
            leverage: 100.0,
            margin: 0.0,
            equity: 0.0,
            free_margin: 0.0,
            margin_level: 0.0,
        };

        let group = TradingGroupInstrumentSettings {
            digits: 5,
            max_leverage: None,
            markup_settings: Some(TradingGroupInstrumentMarkupSettings {
                markup_bid: 0.0,
                markup_ask: 0.0,
                min_spread: Some(0.00020),
                max_spread: None,
            }),
        };

        let (margin, gross) = calculate_specific_instrument_margin_and_gross_pl(
            &position.iter().collect::<Vec<_>>(),
            &account,
            None,
            &group,
        );

        assert_eq!(format!("{:.5}", margin), "62.77000");
    }
}

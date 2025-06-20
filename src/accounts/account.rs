use std::collections::HashMap;

use crate::{
    positions::position::MicroEnginePosition,
    settings::{TradingGroupInstrumentSettings, TradingGroupSettings},
};

pub struct MicroEngineAccountCalculationUpdate {
    pub account_id: String,
    pub margin: f64,
    pub equity: f64,
    pub free_margin: f64,
    pub margin_level: f64,
}

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
        settings: &TradingGroupSettings,
    ) -> MicroEngineAccountCalculationUpdate {
        let (margin, gross_pl) =
            self.calculate_margin_and_gross_pl(account_positions, settings.hedge_coef, settings);

        self.margin = margin;
        self.equity = self.balance + gross_pl;
        self.free_margin = self.equity - self.margin;
        self.margin_level = match margin < 0.00001 {
            true => 0.0,
            false => self.equity / margin * 100.0,
        };

        MicroEngineAccountCalculationUpdate {
            account_id: self.id.clone(),
            margin: self.margin,
            equity: self.equity,
            free_margin: self.free_margin,
            margin_level: self.margin_level,
        }
    }

    fn calculate_margin_and_gross_pl(
        &self,
        account_positions: &[&MicroEnginePosition],
        hedge_coef: Option<f64>,
        settings: &TradingGroupSettings,
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
        match position.is_buy {
            true => {
                buy_margin_price_sum +=
                    position.margin_bidask.get_open_price(position.is_buy) * position.lots_amount;
                buy_volume += position.lots_amount;
                contract_size_sum += position.contract_size;
            }
            false => {
                sell_margin_price_sum +=
                    position.margin_bidask.get_open_price(position.is_buy) * position.lots_amount;
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

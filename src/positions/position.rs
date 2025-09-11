use chrono::{DateTime, Utc};

use crate::{
    bidask::{MicroEngineBidAskCache, dto::MicroEngineBidask},
    settings::MicroEngineTradingGroupSettings,
};

#[derive(Default, Clone, Debug)]
pub struct MicroEnginePositionSwap {
    pub date: DateTime<Utc>,
    pub delta: f64,
}

#[derive(Default, Clone, Debug)]
pub struct MicroEnginePosition {
    pub id: String,
    pub trader_id: String,
    pub account_id: String,
    pub base: String,
    pub quote: String,
    pub collateral: String,
    pub asset_pair: String,
    pub lots_amount: f64,
    pub contract_size: f64,
    pub is_buy: bool,
    pub pl: f64,
    pub commission: f64,
    pub open_bidask: MicroEngineBidask,
    pub active_bidask: MicroEngineBidask,
    pub margin_bidask: MicroEngineBidask,
    pub profit_bidask: MicroEngineBidask,
    pub profit_price_assets_subscriptions: Vec<String>,
    pub swaps_sum: f64,
}

impl MicroEnginePosition {
    pub fn get_gross_pl(&self) -> f64 {
        self.pl - self.commission + self.swaps_sum
    }

    pub fn update_bidask(
        &mut self,
        bidask: &MicroEngineBidask,
        bidask_cache: &MicroEngineBidAskCache,
        settings: &MicroEngineTradingGroupSettings,
    ) {
        let Some(instrument_settings) = settings.instruments.get(&bidask.id) else {
            return;
        };

        let (new_bid, new_ask) = instrument_settings.calculate_bidask(bidask);

        if self.asset_pair == bidask.id {
            self.active_bidask.bid = new_bid;
            self.active_bidask.ask = new_ask;
        }

        let mut profit_hit = false;

        for asset in &self.profit_price_assets_subscriptions {
            if asset == &bidask.id {
                profit_hit = true;
                break;
            }
        }

        if profit_hit {
            if let Some(profit_price) = bidask_cache.get_price(&self.quote, &self.collateral) {
                self.profit_bidask = profit_price
            }
        }

        let open_price = self.open_bidask.get_open_price(self.is_buy);
        let close_price = self.active_bidask.get_close_price(self.is_buy);

        let diff = match self.is_buy {
            true => close_price - open_price,
            false => open_price - close_price,
        };

        let profit_price = match diff >= 0.0 {
            true => self.profit_bidask.bid,
            false => self.profit_bidask.ask,
        };

        self.pl = diff * self.lots_amount * self.contract_size * profit_price;
    }
}

#[cfg(test)]
mod test {
    use std::collections::{HashMap, HashSet};

    use crate::{
        bidask::{MicroEngineBidAskCache, MicroEngineInstrument, dto::MicroEngineBidask},
        positions::position::MicroEnginePosition,
        settings::TradingGroupInstrumentSettings,
    };

    #[tokio::test]
    pub async fn test_pl_calculation_base() {
        let (bidask_cache, _) = MicroEngineBidAskCache::new(
            HashSet::from_iter(vec!["USD".to_string()].into_iter()),
            vec![MicroEngineInstrument {
                id: "EURUSD".to_string(),
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }],
            vec![MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.15173,
                ask: 1.15173,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }],
        );

        let settings = crate::settings::MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            instruments: HashMap::from_iter(
                vec![(
                    "EURUSD".to_string(),
                    TradingGroupInstrumentSettings {
                        digits: 5,
                        max_leverage: None,
                        markup_settings: None,
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
        };

        let mut position = MicroEnginePosition {
            id: "id".to_string(),
            trader_id: "trader_id".to_string(),
            account_id: "account_id".to_string(),
            base: "EUR".to_string(),
            quote: "USD".to_string(),
            collateral: "USD".to_string(),
            asset_pair: "EURUSD".to_string(),
            lots_amount: 0.01,
            contract_size: 100000.0,
            is_buy: true,
            pl: 0.0,
            commission: 0.0,
            open_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.05173,
                ask: 1.05173,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            active_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.05173,
                ask: 1.05173,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            margin_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.05173,
                ask: 1.05173,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            profit_bidask: MicroEngineBidask::create_blank(),
            profit_price_assets_subscriptions: vec![],
            swaps_sum: 0.0,
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.07113,
                ask: 1.07113,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &bidask_cache,
            &settings,
        );

        assert_eq!(format!("{:.5}", position.get_gross_pl()), "19.40000");
    }
}

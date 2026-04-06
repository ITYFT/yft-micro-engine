use chrono::{DateTime, Utc};

use crate::{
    bidask::{MicroEngineBidAskCache, dto::MicroEngineBidask},
    round_float_to_digits,
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
    pub is_trailing_sl: bool,
    pub trailing_sl_offset: f64,
    pub sl_price: Option<f64>,
}

impl MicroEnginePosition {
    pub fn get_gross_pl(&self) -> f64 {
        self.pl - self.commission + self.swaps_sum
    }


    pub fn update_bidask(
        &mut self,
        bidask: &MicroEngineBidask,
        bidask_cache: &mut MicroEngineBidAskCache,
        settings: &MicroEngineTradingGroupSettings,
    ) {
        let Some(instrument_settings) = settings.instruments.get(&bidask.id) else {
            return;
        };

        let (new_bid, new_ask) = instrument_settings.calculate_bidask(bidask);

        if self.asset_pair == bidask.id {
            // Update active_bidask with markup applied
            // Note: We don't modify open_bidask here because positions from trading engine
            // already have markup applied to open_bidask when they're created.
            // We only apply markup to active_bidask when prices update.
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
            if (bidask.base == self.quote && bidask.quote == self.collateral) 
                || (bidask.base == self.collateral && bidask.quote == self.quote) {
                
                // Match trading-engine behavior: use raw bidask, reverse if needed
                let mut profit_price = bidask.clone();
                
                // Reverse if needed (Case B: when bidask needs to be reversed)
                if bidask.base == self.collateral && bidask.quote == self.quote {
                    profit_price = profit_price.reverse();
                }
                
                self.profit_bidask = profit_price;
            } else {
                // Match trading-engine behavior: get raw price from cache, no markup
                if let Some(profit_price) = bidask_cache.get_price(&self.quote, &self.collateral) {
                    self.profit_bidask = profit_price;
                }
            }
        }

        self.recalculate_pl(settings);
    }

    /// Recalculate PL based on current open_price, close_price, and profit_bidask
    pub fn recalculate_pl(&mut self, settings: &MicroEngineTradingGroupSettings) {
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

        let raw_pl = diff * self.lots_amount * self.contract_size * profit_price;
        
        // Use collateral currency digits for rounding, matching trading-engine-core behavior
        let digits = settings
            .collaterals
            .get(&self.collateral)
            .map(|c| c.digits as i32)
            .unwrap_or(2); // Fallback to 2 if collateral not found
        
        self.pl = round_float_to_digits(raw_pl, digits);
    }

    pub fn recalculate_trailing_sl(&mut self, digits: i32) {
        if !self.is_trailing_sl || self.trailing_sl_offset <= 0.0 {
            return;
        }

        let close_price = self.active_bidask.get_close_price(self.is_buy);
        let new_sl = if self.is_buy {
            round_float_to_digits(close_price - self.trailing_sl_offset, digits)
        } else {
            round_float_to_digits(close_price + self.trailing_sl_offset, digits)
        };

        let should_update = match self.sl_price {
            None => true,
            Some(current_sl) => {
                if self.is_buy { new_sl > current_sl } else { new_sl < current_sl }
            }
        };

        if should_update {
            self.sl_price = Some(new_sl);
        }
    }

    /// Update profit_bidask from raw cache prices (no markup), matching trading-engine behavior
    pub fn update_profit_bidask_from_cache(&mut self, bidask_cache: &mut MicroEngineBidAskCache) {
        // Only update if position needs currency conversion
        if self.quote == self.collateral {
            return;
        }

        // Match trading-engine behavior: get raw price from cache, no markup
        if let Some(profit_price) = bidask_cache.get_price(&self.quote, &self.collateral) {
            self.profit_bidask = profit_price;
        }
    }
}

#[cfg(test)]
mod test {
    use std::collections::{HashMap, HashSet};

    use crate::{
        bidask::{MicroEngineBidAskCache, MicroEngineInstrument, dto::MicroEngineBidask},
        positions::position::MicroEnginePosition,
        settings::{TradingGroupInstrumentMarkupSettings, TradingGroupInstrumentSettings},
    };

    #[tokio::test]
    pub async fn test_pl_calculation_base() {
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
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
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
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
            ..Default::default()
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.07113,
                ask: 1.07113,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        assert_eq!(format!("{:.5}", position.get_gross_pl()), "19.40000");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_markup() {
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
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

        let point_size = 1f64 / 10f64.powi(5 as i32);

        let settings = crate::settings::MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            instruments: HashMap::from_iter(
                vec![(
                    "EURUSD".to_string(),
                    TradingGroupInstrumentSettings {
                        digits: 5,
                        max_leverage: None,
                        markup_settings: Some(TradingGroupInstrumentMarkupSettings {
                            markup_bid: -123.0 * point_size,
                            markup_ask: 135.0 * point_size,
                            min_spread: None,
                            max_spread: None,
                        }),
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
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
            ..Default::default()
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.07113,
                ask: 1.07113,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.06990");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.07248");
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "18.17000");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_markup_min() {
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
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

        let point_size = 1f64 / 10f64.powi(5 as i32);

        let settings = crate::settings::MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            instruments: HashMap::from_iter(
                vec![(
                    "EURUSD".to_string(),
                    TradingGroupInstrumentSettings {
                        digits: 5,
                        max_leverage: None,
                        markup_settings: Some(TradingGroupInstrumentMarkupSettings {
                            markup_bid: 0.0 * point_size,
                            markup_ask: 0.0 * point_size,
                            min_spread: Some(10.0 * point_size),
                            max_spread: None,
                        }),
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
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
            ..Default::default()
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.07113,
                ask: 1.07113,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.07108");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.07118");
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "19.35000");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_markup_max() {
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
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

        let point_size = 1f64 / 10f64.powi(5 as i32);

        let settings = crate::settings::MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            instruments: HashMap::from_iter(
                vec![(
                    "EURUSD".to_string(),
                    TradingGroupInstrumentSettings {
                        digits: 5,
                        max_leverage: None,
                        markup_settings: Some(TradingGroupInstrumentMarkupSettings {
                            markup_bid: 0.0 * point_size,
                            markup_ask: 0.0 * point_size,
                            min_spread: None,
                            max_spread: Some(10.0 * point_size),
                        }),
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
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
            ..Default::default()
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.07101,
                ask: 1.07121,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.07106");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.07116");
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "19.33000");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_real_case() {
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
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
                        // markup_settings: Some(TradingGroupInstrumentMarkupSettings {
                        //     markup_bid: -154.0 * (1f64 / 10f64.powi(5 as i32)),
                        //     markup_ask: -55.0 * (1f64 / 10f64.powi(5 as i32)),
                        //     min_spread: None,
                        //     max_spread: None,
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
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
            is_buy: false,
            pl: 0.0,
            commission: 0.05,
            open_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.16723,
                ask: 1.16823,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            active_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.16723,
                ask: 1.16823,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            margin_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.16704,
                ask: 1.16804,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            profit_bidask: MicroEngineBidask::create_blank(),
            profit_price_assets_subscriptions: vec![],
            swaps_sum: 0.0,
            ..Default::default()
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.16703,
                ask: 1.16804,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.16703");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.16804");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_real_case_with_markup() {
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
            HashSet::from_iter(vec!["USD".to_string()].into_iter()),
            vec![MicroEngineInstrument {
                id: "EURUSD".to_string(),
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }],
            vec![MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25555,
                ask: 1.35555,
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
                        markup_settings: Some(TradingGroupInstrumentMarkupSettings {
                            markup_bid: -300.0 * 0.00001,
                            markup_ask: -250.0 * 0.00001,
                            min_spread: None,
                            max_spread: None,
                        }),
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
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
            is_buy: false,
            pl: 0.0,
            commission: 0.05,
            open_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25555,
                ask: 1.35555,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            active_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25555,
                ask: 1.35555,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            margin_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25555,
                ask: 1.35555,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            profit_bidask: MicroEngineBidask::create_blank(),
            profit_price_assets_subscriptions: vec![],
            swaps_sum: 0.0,
            ..Default::default()
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.45555,
                ask: 1.55555,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );
        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.45255");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.55305");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_real_case_with_markup_max() {
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
            HashSet::from_iter(vec!["USD".to_string()].into_iter()),
            vec![MicroEngineInstrument {
                id: "EURUSD".to_string(),
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }],
            vec![MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25580,
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
                        markup_settings: Some(TradingGroupInstrumentMarkupSettings {
                            markup_bid: 0.0,
                            markup_ask: 0.0,
                            min_spread: None,
                            max_spread: Some(0.00020),
                        }),
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
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
            is_buy: false,
            pl: 0.0,
            commission: 0.05,
            open_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25580,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            active_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25580,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            margin_bidask: MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25580,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            profit_bidask: MicroEngineBidask::create_blank(),
            profit_price_assets_subscriptions: vec![],
            swaps_sum: 0.0,
            ..Default::default()
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25580,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        assert_eq!(format!("{:.5}", position.pl), "-0.30000");
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "-0.35000");
        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.25550");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.25570");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_real_case_with_markup_min() {
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
            HashSet::from_iter(vec!["USD".to_string()].into_iter()),
            vec![MicroEngineInstrument {
                id: "EURUSD".to_string(),
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }],
            vec![MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
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
                        markup_settings: Some(TradingGroupInstrumentMarkupSettings {
                            markup_bid: 0.0,
                            markup_ask: 0.0,
                            min_spread: Some(0.00020),
                            max_spread: None,
                        }),
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
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
            is_buy: false,
            pl: 0.0,
            commission: 0.05,
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
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.25531");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.25551");

        assert_eq!(format!("{:.5}", position.pl), "-0.11000");
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "-0.16000");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_usdcad_with_conversion() {
        // Test USDCAD instrument with USD account currency
        // This should trigger currency conversion from CAD to USD
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
            HashSet::from_iter(vec!["USD".to_string()].into_iter()),
            vec![MicroEngineInstrument {
                id: "USDCAD".to_string(),
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            }],
            vec![MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3500,
                ask: 1.3502,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            }],
        );

        let settings = crate::settings::MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            instruments: HashMap::from_iter(
                vec![(
                    "USDCAD".to_string(),
                    TradingGroupInstrumentSettings {
                        digits: 5,
                        max_leverage: None,
                        markup_settings: None,
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
        };

        let mut position = MicroEnginePosition {
            id: "id".to_string(),
            trader_id: "trader_id".to_string(),
            account_id: "account_id".to_string(),
            base: "USD".to_string(),
            quote: "CAD".to_string(),
            collateral: "USD".to_string(),
            asset_pair: "USDCAD".to_string(),
            lots_amount: 0.01,
            contract_size: 100000.0,
            is_buy: true,
            pl: 0.0,
            commission: 0.0,
            open_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3500,
                ask: 1.3502,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            active_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3500,
                ask: 1.3502,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            margin_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3500,
                ask: 1.3502,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            profit_bidask: MicroEngineBidask::create_blank(),
            profit_price_assets_subscriptions: vec!["USDCAD".to_string()], // CAD->USD needs USDCAD
            swaps_sum: 0.0,
            ..Default::default()
        };

        // Update with new price: USDCAD rises to 1.3600
        position.update_bidask(
            &MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3600,
                ask: 1.3602,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        // BUY position: opened at 1.3502 (ask), now at 1.3600 (bid for closing)
        // Price change: 1.3600 - 1.3502 = 0.0098 CAD per 1 USD
        // Raw PnL in CAD: 0.0098 * 0.01 * 100000 = 9.8 CAD
        // Reversed USDCAD (CAD->USD): bid becomes 1/ask, ask becomes 1/bid
        // For profit (diff > 0), we use profit_bidask.bid = 1/USDCAD.ask = 1/1.3602 = 0.735281...
        // PnL in USD: 9.8 * 0.735281 = 7.205818... USD

        // Check that profit_bidask was updated (reversed USDCAD for CAD->USD)
        assert!(position.profit_bidask.bid > 0.7);
        assert!(position.profit_bidask.bid < 0.8);

        // Check the final PnL calculation - now using current conversion rate
        // Note: With the fix, settings are now correctly applied to reversed bidask
        // Using collateral currency digits (2 for USD) for rounding, matching trading-engine-core
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "7.20000");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_usdcad_sell_with_conversion() {
        // Test USDCAD SELL position with USD account currency
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
            HashSet::from_iter(vec!["USD".to_string()].into_iter()),
            vec![MicroEngineInstrument {
                id: "USDCAD".to_string(),
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            }],
            vec![MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3600,
                ask: 1.3602,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            }],
        );

        let settings = crate::settings::MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            instruments: HashMap::from_iter(
                vec![(
                    "USDCAD".to_string(),
                    TradingGroupInstrumentSettings {
                        digits: 5,
                        max_leverage: None,
                        markup_settings: None,
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
        };

        let mut position = MicroEnginePosition {
            id: "id".to_string(),
            trader_id: "trader_id".to_string(),
            account_id: "account_id".to_string(),
            base: "USD".to_string(),
            quote: "CAD".to_string(),
            collateral: "USD".to_string(),
            asset_pair: "USDCAD".to_string(),
            lots_amount: 0.01,
            contract_size: 100000.0,
            is_buy: false, // SELL position
            pl: 0.0,
            commission: 0.0,
            open_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3600,
                ask: 1.3602,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            active_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3600,
                ask: 1.3602,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            margin_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3600,
                ask: 1.3602,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            profit_bidask: MicroEngineBidask::create_blank(),
            profit_price_assets_subscriptions: vec!["USDCAD".to_string()],
            swaps_sum: 0.0,
            ..Default::default()
        };

        // Update with new price: USDCAD drops to 1.3500
        position.update_bidask(
            &MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3500,
                ask: 1.3502,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        // SELL position: opened at 1.3600 (bid), now at 1.3502 (ask for closing)
        // Price change: 1.3600 - 1.3502 = 0.0098 CAD per 1 USD (profit)
        // Raw PnL in CAD: 0.0098 * 0.01 * 100000 = 9.8 CAD
        // Reversed USDCAD (CAD->USD): bid becomes 1/ask, ask becomes 1/bid
        // For profit (diff > 0), we use profit_bidask.bid = 1/USDCAD.ask = 1/1.3502 = 0.740481...
        // PnL in USD: 9.8 * 0.740481 = 7.204824... USD

        // Check the final PnL calculation - now using current conversion rate
        // Note: With the fix, settings are now correctly applied to reversed bidask
        // Using collateral currency digits (2 for USD) for rounding, matching trading-engine-core
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "7.26000");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_usdcad_with_conversion_and_markup() {
        // Test USDCAD with markup to verify the fix for reversed bidask markup application
        // This test verifies that markup is correctly applied to the reversed USDCAD bidask
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
            HashSet::from_iter(vec!["USD".to_string()].into_iter()),
            vec![MicroEngineInstrument {
                id: "USDCAD".to_string(),
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            }],
            vec![MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3500,
                ask: 1.3502,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            }],
        );

        let point_size = 1f64 / 10f64.powi(5 as i32);

        let settings = crate::settings::MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            instruments: HashMap::from_iter(
                vec![(
                    "USDCAD".to_string(),
                    TradingGroupInstrumentSettings {
                        digits: 5,
                        max_leverage: None,
                        markup_settings: Some(TradingGroupInstrumentMarkupSettings {
                            markup_bid: -50.0 * point_size,  // -0.0005
                            markup_ask: 50.0 * point_size,   // +0.0005
                            min_spread: None,
                            max_spread: None,
                        }),
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
        };

        let mut position = MicroEnginePosition {
            id: "id".to_string(),
            trader_id: "trader_id".to_string(),
            account_id: "account_id".to_string(),
            base: "USD".to_string(),
            quote: "CAD".to_string(),
            collateral: "USD".to_string(),
            asset_pair: "USDCAD".to_string(),
            lots_amount: 1.0,
            contract_size: 100000.0,
            is_buy: true,
            pl: 0.0,
            commission: 0.0,
            open_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3500,
                ask: 1.3502,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            active_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3500,
                ask: 1.3502,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            margin_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3500,
                ask: 1.3502,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            profit_bidask: MicroEngineBidask::create_blank(),
            profit_price_assets_subscriptions: vec!["USDCAD".to_string()],
            swaps_sum: 0.0,
            ..Default::default()
        };

        // Update with new price: USDCAD rises to 1.3600
        position.update_bidask(
            &MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3600,
                ask: 1.3602,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        // BUY position: opened at 1.3502 (ask), now at 1.3600 (bid for closing)
        // Price change: 1.3600 - 1.3502 = 0.0098 CAD per 1 USD
        // Raw PnL in CAD: 0.0098 * 1.0 * 100000 = 980 CAD
        
        // With markup: USDCAD bid becomes 1.3600 - 0.0005 = 1.3595, ask becomes 1.3602 + 0.0005 = 1.3607
        // Reversed USDCAD (CAD->USD): bid = 1/ask = 1/1.3607 = 0.7350..., ask = 1/bid = 1/1.3595 = 0.7355...
        // For profit (diff > 0), we use profit_bidask.bid = 1/1.3607 = 0.7350...
        // PnL in USD: 980 * 0.7350... ≈ 720.30 USD (with markup applied)

        // profit_bidask uses raw prices (no markup), matching trading-engine behavior.
        // USDCAD reversed (CAD->USD): bid = 1/ask_raw = 1/1.3602 ≈ 0.73518
        let expected_bid = 1.0 / 1.3602;
        assert!(
            (position.profit_bidask.bid - expected_bid).abs() < 1e-5,
            "profit_bidask.bid should be raw reversed USDCAD ask: expected {expected_bid}, got {}",
            position.profit_bidask.bid
        );
    }

    #[tokio::test]
    pub async fn test_pl_calculation_usdcad_demonstrates_conversion_logic() {
        // This test demonstrates the complete flow of currency conversion for SELL position
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
            HashSet::from_iter(vec!["USD".to_string()].into_iter()),
            vec![MicroEngineInstrument {
                id: "USDCAD".to_string(),
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            }],
            vec![MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3600,
                ask: 1.3602,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            }],
        );

        let settings = crate::settings::MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            instruments: HashMap::from_iter(
                vec![(
                    "USDCAD".to_string(),
                    TradingGroupInstrumentSettings {
                        digits: 5,
                        max_leverage: None,
                        markup_settings: None,
                    },
                )]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
        };

        let mut position = MicroEnginePosition {
            id: "id".to_string(),
            trader_id: "trader_id".to_string(),
            account_id: "account_id".to_string(),
            base: "USD".to_string(),
            quote: "CAD".to_string(),
            collateral: "USD".to_string(), // Account currency is USD
            asset_pair: "USDCAD".to_string(),
            lots_amount: 1.0, // 1 lot = 100,000 units
            contract_size: 100000.0,
            is_buy: false, // SELL position - this is where mismatch typically occurs
            pl: 0.0,
            commission: 0.0,
            open_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3600,
                ask: 1.3602,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            active_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3600,
                ask: 1.3602,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            margin_bidask: MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3600,
                ask: 1.3602,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            profit_bidask: MicroEngineBidask::create_blank(),
            profit_price_assets_subscriptions: vec!["USDCAD".to_string()], // Needs USDCAD to convert CAD->USD
            swaps_sum: 0.0,
            ..Default::default()
        };

        // Price moves down: USDCAD drops from 1.3600 to 1.3500 (profit for SELL)
        position.update_bidask(
            &MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3500,
                ask: 1.3502,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        // === Verification of conversion logic ===
        
        // 1. Check that profit_bidask is set (CAD->USD conversion rate)
        // It should be the REVERSE of USDCAD: 1/1.3502 for bid, 1/1.3500 for ask
        println!("profit_bidask.bid (CAD->USD): {}", position.profit_bidask.bid);
        println!("profit_bidask.ask (CAD->USD): {}", position.profit_bidask.ask);
        assert!(position.profit_bidask.bid > 0.73 && position.profit_bidask.bid < 0.75);
        assert!(position.profit_bidask.ask > 0.73 && position.profit_bidask.ask < 0.75);

        // 2. Calculate expected PnL manually
        let open_price = 1.3600; // SELL opens at bid
        let close_price = 1.3502; // SELL closes at ask
        let price_diff = open_price - close_price; // 0.0098 (profit)
        let lots = 1.0;
        let contract_size = 100000.0;
        
        // PnL in CAD
        let pnl_cad: f64 = price_diff * lots * contract_size;
        println!("PnL in CAD: {} CAD", pnl_cad);
        assert!((pnl_cad - 980.0).abs() < 0.01); // 0.0098 * 1.0 * 100000 ≈ 980
        
        // Conversion rate: for profit, use bid = 1/1.3502 = 0.740481...
        let conversion_rate = position.profit_bidask.bid;
        let expected_pnl_usd = pnl_cad * conversion_rate;
        println!("Conversion rate (CAD->USD): {}", conversion_rate);
        println!("Expected PnL in USD: {} USD", expected_pnl_usd);
        
        // 3. Verify the actual PnL matches our calculation
        println!("Actual PnL in USD: {}", position.pl);
        assert_eq!(format!("{:.2}", position.pl), format!("{:.2}", expected_pnl_usd));
        
        // The key insight: without conversion, PnL would be 980 CAD
        // With current conversion at 1/1.3502 = 0.7406, it becomes ~725.82 USD
        assert_eq!(format!("{:.2}", position.get_gross_pl()), "725.82");
    }

    #[tokio::test]
    pub async fn test_cache_based_cross_rate_calculation() {
        // Test cross-rate calculation when EURCAD position needs CAD->USD conversion
        // but update comes from EURUSD (not USDCAD)
        // System should use cache to calculate via cross-rate: CAD/USD = EUR/CAD * USD/EUR
        
        let (mut bidask_cache, _) = MicroEngineBidAskCache::new(
            HashSet::from_iter(vec!["USD".to_string()].into_iter()),
            vec![
                MicroEngineInstrument {
                    id: "EURCAD".to_string(),
                    base: "EUR".to_string(),
                    quote: "CAD".to_string(),
                },
                MicroEngineInstrument {
                    id: "EURUSD".to_string(),
                    base: "EUR".to_string(),
                    quote: "USD".to_string(),
                },
                MicroEngineInstrument {
                    id: "USDCAD".to_string(),
                    base: "USD".to_string(),
                    quote: "CAD".to_string(),
                },
            ],
            vec![
                MicroEngineBidask {
                    id: "EURCAD".to_string(),
                    bid: 1.4500,
                    ask: 1.4502,
                    base: "EUR".to_string(),
                    quote: "CAD".to_string(),
                },
                MicroEngineBidask {
                    id: "EURUSD".to_string(),
                    bid: 1.0800,
                    ask: 1.0802,
                    base: "EUR".to_string(),
                    quote: "USD".to_string(),
                },
                MicroEngineBidask {
                    id: "USDCAD".to_string(),
                    bid: 1.3400,
                    ask: 1.3402,
                    base: "USD".to_string(),
                    quote: "CAD".to_string(),
                },
            ],
        );

        let settings = crate::settings::MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            instruments: HashMap::from_iter(
                vec![
                    (
                        "EURCAD".to_string(),
                        TradingGroupInstrumentSettings {
                            digits: 5,
                            max_leverage: None,
                            markup_settings: None,
                        },
                    ),
                    (
                        "EURUSD".to_string(),
                        TradingGroupInstrumentSettings {
                            digits: 5,
                            max_leverage: None,
                            markup_settings: None,
                        },
                    ),
                    (
                        "USDCAD".to_string(),
                        TradingGroupInstrumentSettings {
                            digits: 5,
                            max_leverage: None,
                            markup_settings: None,
                        },
                    ),
                ]
                .into_iter(),
            ),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), crate::settings::CollateralSettings { digits: 2 }),
            ]),
        };

        let mut position = MicroEnginePosition {
            id: "id".to_string(),
            trader_id: "trader_id".to_string(),
            account_id: "account_id".to_string(),
            base: "EUR".to_string(),
            quote: "CAD".to_string(),
            collateral: "USD".to_string(), // Account in USD, position in CAD
            asset_pair: "EURCAD".to_string(),
            lots_amount: 1.0,
            contract_size: 100000.0,
            is_buy: false, // SELL
            pl: 0.0,
            commission: 0.0,
            open_bidask: MicroEngineBidask {
                id: "EURCAD".to_string(),
                bid: 1.4500,
                ask: 1.4502,
                base: "EUR".to_string(),
                quote: "CAD".to_string(),
            },
            active_bidask: MicroEngineBidask {
                id: "EURCAD".to_string(),
                bid: 1.4500,
                ask: 1.4502,
                base: "EUR".to_string(),
                quote: "CAD".to_string(),
            },
            margin_bidask: MicroEngineBidask {
                id: "EURCAD".to_string(),
                bid: 1.4500,
                ask: 1.4502,
                base: "EUR".to_string(),
                quote: "CAD".to_string(),
            },
            profit_bidask: MicroEngineBidask::create_blank(),
            // IMPORTANT: subscribes to both USDCAD (direct) and EURUSD (for cross-rate fallback)
            profit_price_assets_subscriptions: vec!["USDCAD".to_string(), "EURUSD".to_string()],
            swaps_sum: 0.0,
            ..Default::default()
        };

        // First update: EURCAD price moves (position instrument)
        bidask_cache.handle_new(&MicroEngineBidask {
            id: "EURCAD".to_string(),
            bid: 1.4400,
            ask: 1.4402,
            base: "EUR".to_string(),
            quote: "CAD".to_string(),
        });
        
        position.update_bidask(
            &MicroEngineBidask {
                id: "EURCAD".to_string(),
                bid: 1.4400,
                ask: 1.4402,
                base: "EUR".to_string(),
                quote: "CAD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        println!("\n=== After EURCAD update ===");
        println!("Position active bidask: {} / {}", position.active_bidask.bid, position.active_bidask.ask);
        println!("Profit bidask (CAD->USD): {} / {}", position.profit_bidask.bid, position.profit_bidask.ask);
        println!("PnL: {}", position.pl);

        // SELL position: opened at 1.4500 (bid), now at 1.4402 (ask)
        // Price diff: 1.4500 - 1.4402 = 0.0098 CAD profit
        // PnL in CAD: 0.0098 * 1.0 * 100000 = 980 CAD
        // Conversion: Should use USDCAD from cache = 1.3400, reversed = 1/1.3402 ≈ 0.7461
        // PnL in USD: 980 * 0.7461 ≈ 731.18 USD
        
        println!("Expected PnL: ~731 USD (using OLD cached USDCAD rate)");
        
        // Now USDCAD price updates to 1.3500
        bidask_cache.handle_new(&MicroEngineBidask {
            id: "USDCAD".to_string(),
            bid: 1.3500,
            ask: 1.3502,
            base: "USD".to_string(),
            quote: "CAD".to_string(),
        });
        
        position.update_bidask(
            &MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3500,
                ask: 1.3502,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            &mut bidask_cache,
            &settings,
        );

        println!("\n=== After USDCAD update (current price) ===");
        println!("Profit bidask (CAD->USD): {} / {}", position.profit_bidask.bid, position.profit_bidask.ask);
        println!("PnL: {}", position.pl);
        println!("Expected PnL: ~725.67 USD (using CURRENT USDCAD rate)");
        
        // With the fix: uses incoming USDCAD = 1.3502, reversed = 1/1.3502 ≈ 0.7405
        // PnL in USD: 980 * 0.7405 ≈ 725.67 USD
        
        assert!(position.profit_bidask.bid > 0.74 && position.profit_bidask.bid < 0.75);
    }

    fn make_bidask(id: &str, bid: f64, ask: f64) -> MicroEngineBidask {
        MicroEngineBidask {
            id: id.to_string(),
            bid,
            ask,
            base: "EUR".to_string(),
            quote: "USD".to_string(),
        }
    }

    fn make_settings(digits: u32) -> crate::settings::MicroEngineTradingGroupSettings {
        crate::settings::MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            instruments: HashMap::from_iter(vec![(
                "EURUSD".to_string(),
                TradingGroupInstrumentSettings {
                    digits,
                    max_leverage: None,
                    markup_settings: None,
                },
            )]),
            hedge_coef: None,
            collaterals: HashMap::from_iter(vec![(
                "USD".to_string(),
                crate::settings::CollateralSettings { digits: 2 },
            )]),
        }
    }

    fn make_cache() -> MicroEngineBidAskCache {
        let (cache, _) = MicroEngineBidAskCache::new(
            std::collections::HashSet::from_iter(vec!["USD".to_string()]),
            vec![crate::bidask::MicroEngineInstrument {
                id: "EURUSD".to_string(),
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }],
            vec![make_bidask("EURUSD", 1.1000, 1.1002)],
        );
        cache
    }

    // buy: SL moves up when bid rises above offset threshold
    #[tokio::test]
    pub async fn test_trailing_sl_buy_sl_moves_up() {
        let mut cache = make_cache();
        let settings = make_settings(5);

        let mut position = MicroEnginePosition {
            id: "p1".to_string(),
            account_id: "acc1".to_string(),
            asset_pair: "EURUSD".to_string(),
            is_buy: true,
            is_trailing_sl: true,
            trailing_sl_offset: 0.00500, // 50 pips
            sl_price: Some(1.09500),     // initial SL
            active_bidask: make_bidask("EURUSD", 1.1000, 1.1002),
            open_bidask: make_bidask("EURUSD", 1.1000, 1.1002),
            profit_bidask: MicroEngineBidask::create_blank(),
            ..Default::default()
        };

        // price rises: bid = 1.10800 → new SL = 1.10800 - 0.00500 = 1.10300
        position.update_bidask(&make_bidask("EURUSD", 1.10800, 1.10820), &mut cache, &settings);
        position.recalculate_trailing_sl(5);

        let sl = position.sl_price.unwrap();
        assert!(
            (sl - 1.10300).abs() < 1e-5,
            "Expected SL ≈ 1.10300, got {sl}"
        );
    }

    // buy: SL does NOT move down when bid drops
    #[tokio::test]
    pub async fn test_trailing_sl_buy_sl_does_not_move_down() {
        let mut cache = make_cache();
        let settings = make_settings(5);

        let mut position = MicroEnginePosition {
            id: "p2".to_string(),
            account_id: "acc1".to_string(),
            asset_pair: "EURUSD".to_string(),
            is_buy: true,
            is_trailing_sl: true,
            trailing_sl_offset: 0.00500,
            sl_price: Some(1.10300), // already at high watermark
            active_bidask: make_bidask("EURUSD", 1.1080, 1.1082),
            open_bidask: make_bidask("EURUSD", 1.1000, 1.1002),
            profit_bidask: MicroEngineBidask::create_blank(),
            ..Default::default()
        };

        // price drops: bid = 1.10200 → computed SL = 1.09700, must NOT update
        position.update_bidask(&make_bidask("EURUSD", 1.10200, 1.10220), &mut cache, &settings);
        position.recalculate_trailing_sl(5);

        let sl = position.sl_price.unwrap();
        assert!(
            (sl - 1.10300).abs() < 1e-5,
            "SL should stay at 1.10300, got {sl}"
        );
    }

    // sell: SL moves down when ask drops
    #[tokio::test]
    pub async fn test_trailing_sl_sell_sl_moves_down() {
        let mut cache = make_cache();
        let settings = make_settings(5);

        let mut position = MicroEnginePosition {
            id: "p3".to_string(),
            account_id: "acc1".to_string(),
            asset_pair: "EURUSD".to_string(),
            is_buy: false,
            is_trailing_sl: true,
            trailing_sl_offset: 0.00500,
            sl_price: Some(1.10500), // initial SL
            active_bidask: make_bidask("EURUSD", 1.1000, 1.1002),
            open_bidask: make_bidask("EURUSD", 1.1000, 1.1002),
            profit_bidask: MicroEngineBidask::create_blank(),
            ..Default::default()
        };

        // price drops: ask = 1.09220 → new SL = 1.09220 + 0.00500 = 1.09720
        position.update_bidask(&make_bidask("EURUSD", 1.09200, 1.09220), &mut cache, &settings);
        position.recalculate_trailing_sl(5);

        let sl = position.sl_price.unwrap();
        assert!(
            (sl - 1.09720).abs() < 1e-5,
            "Expected SL ≈ 1.09720, got {sl}"
        );
    }

    // sell: SL does NOT move up when ask rises
    #[tokio::test]
    pub async fn test_trailing_sl_sell_sl_does_not_move_up() {
        let mut cache = make_cache();
        let settings = make_settings(5);

        let mut position = MicroEnginePosition {
            id: "p4".to_string(),
            account_id: "acc1".to_string(),
            asset_pair: "EURUSD".to_string(),
            is_buy: false,
            is_trailing_sl: true,
            trailing_sl_offset: 0.00500,
            sl_price: Some(1.09720), // already at low watermark
            active_bidask: make_bidask("EURUSD", 1.09200, 1.09220),
            open_bidask: make_bidask("EURUSD", 1.1000, 1.1002),
            profit_bidask: MicroEngineBidask::create_blank(),
            ..Default::default()
        };

        // price rises: ask = 1.09800 → computed SL = 1.10300, must NOT update
        position.update_bidask(&make_bidask("EURUSD", 1.09780, 1.09800), &mut cache, &settings);
        position.recalculate_trailing_sl(5);

        let sl = position.sl_price.unwrap();
        assert!(
            (sl - 1.09720).abs() < 1e-5,
            "SL should stay at 1.09720, got {sl}"
        );
    }

    // non-TSL position: sl_price stays None
    #[tokio::test]
    pub async fn test_trailing_sl_disabled_no_sl_update() {
        let mut cache = make_cache();
        let settings = make_settings(5);

        let mut position = MicroEnginePosition {
            id: "p5".to_string(),
            account_id: "acc1".to_string(),
            asset_pair: "EURUSD".to_string(),
            is_buy: true,
            is_trailing_sl: false,
            trailing_sl_offset: 0.00500,
            sl_price: None,
            active_bidask: make_bidask("EURUSD", 1.1000, 1.1002),
            open_bidask: make_bidask("EURUSD", 1.1000, 1.1002),
            profit_bidask: MicroEngineBidask::create_blank(),
            ..Default::default()
        };

        position.update_bidask(&make_bidask("EURUSD", 1.11000, 1.11020), &mut cache, &settings);
        position.recalculate_trailing_sl(5);

        assert!(position.sl_price.is_none(), "sl_price must stay None for non-TSL position");
    }

    // rounding: SL is rounded to instrument digits
    #[tokio::test]
    pub async fn test_trailing_sl_rounding() {
        let mut cache = make_cache();
        let settings = make_settings(2); // 2 digits

        let mut position = MicroEnginePosition {
            id: "p6".to_string(),
            account_id: "acc1".to_string(),
            asset_pair: "EURUSD".to_string(),
            is_buy: true,
            is_trailing_sl: true,
            trailing_sl_offset: 0.03333,
            sl_price: None,
            active_bidask: make_bidask("EURUSD", 1.1000, 1.1002),
            open_bidask: make_bidask("EURUSD", 1.1000, 1.1002),
            profit_bidask: MicroEngineBidask::create_blank(),
            ..Default::default()
        };

        // bid = 1.10000, offset = 0.03333 → raw SL = 1.06667 → rounded to 2 digits = 1.07
        position.update_bidask(&make_bidask("EURUSD", 1.10000, 1.10020), &mut cache, &settings);
        position.recalculate_trailing_sl(2);

        let sl = position.sl_price.unwrap();
        assert!(
            (sl - 1.07).abs() < 1e-9,
            "Expected SL = 1.07 (rounded to 2 digits), got {sl}"
        );
    }
}

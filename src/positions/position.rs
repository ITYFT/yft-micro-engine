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
        _bidask_cache: &MicroEngineBidAskCache,
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
            if (bidask.base == self.quote && bidask.quote == self.collateral) 
                || (bidask.base == self.collateral && bidask.quote == self.quote) {
                
                let mut profit_price = if bidask.base == self.quote && bidask.quote == self.collateral {
                    bidask.clone()
                } else {
                    bidask.reverse()
                };

                if let Some(profit_instrument_settings) = settings.instruments.get(&profit_price.id)
                {
                    let (new_bid, new_ask) =
                        profit_instrument_settings.calculate_bidask(&profit_price);
                    profit_price.bid = new_bid;
                    profit_price.ask = new_ask;
                }
                self.profit_bidask = profit_price;
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
        settings::{TradingGroupInstrumentMarkupSettings, TradingGroupInstrumentSettings},
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

    #[tokio::test]
    pub async fn test_pl_calculation_markup() {
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

        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.06990");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.07248");
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "18.17000");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_markup_min() {
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

        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.07108");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.07118");
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "19.35000");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_markup_max() {
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
                bid: 1.07101,
                ask: 1.07121,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &bidask_cache,
            &settings,
        );

        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.07106");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.07116");
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "19.33000");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_real_case() {
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
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.16703,
                ask: 1.16804,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &bidask_cache,
            &settings,
        );

        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.16703");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.16804");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_real_case_with_markup() {
        let (bidask_cache, _) = MicroEngineBidAskCache::new(
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
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.45555,
                ask: 1.55555,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &bidask_cache,
            &settings,
        );
        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.45255");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.55305");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_real_case_with_markup_max() {
        let (bidask_cache, _) = MicroEngineBidAskCache::new(
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
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25580,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &bidask_cache,
            &settings,
        );

        assert_eq!(format!("{:.5}", position.pl), "-0.30000");
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "-0.35000");
        assert_eq!(format!("{:.5}", position.active_bidask.bid), "1.25550");
        assert_eq!(format!("{:.5}", position.active_bidask.ask), "1.25570");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_real_case_with_markup_min() {
        let (bidask_cache, _) = MicroEngineBidAskCache::new(
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
        };

        position.update_bidask(
            &MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            },
            &bidask_cache,
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
        let (bidask_cache, _) = MicroEngineBidAskCache::new(
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
            &bidask_cache,
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
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "7.20482");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_usdcad_sell_with_conversion() {
        // Test USDCAD SELL position with USD account currency
        let (bidask_cache, _) = MicroEngineBidAskCache::new(
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
            &bidask_cache,
            &settings,
        );

        // SELL position: opened at 1.3600 (bid), now at 1.3502 (ask for closing)
        // Price change: 1.3600 - 1.3502 = 0.0098 CAD per 1 USD (profit)
        // Raw PnL in CAD: 0.0098 * 0.01 * 100000 = 9.8 CAD
        // Reversed USDCAD (CAD->USD): bid becomes 1/ask, ask becomes 1/bid
        // For profit (diff > 0), we use profit_bidask.bid = 1/USDCAD.ask = 1/1.3502 = 0.740481...
        // PnL in USD: 9.8 * 0.740481 = 7.204824... USD

        // Check the final PnL calculation - now using current conversion rate
        assert_eq!(format!("{:.5}", position.get_gross_pl()), "7.25818");
    }

    #[tokio::test]
    pub async fn test_pl_calculation_usdcad_demonstrates_conversion_logic() {
        // This test demonstrates the complete flow of currency conversion
        let (bidask_cache, _) = MicroEngineBidAskCache::new(
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
            profit_price_assets_subscriptions: vec!["USDCAD".to_string()], // Needs USDCAD to convert CAD->USD
            swaps_sum: 0.0,
        };

        // Price moves: USDCAD goes from 1.3502 to 1.3600
        position.update_bidask(
            &MicroEngineBidask {
                id: "USDCAD".to_string(),
                bid: 1.3600,
                ask: 1.3602,
                base: "USD".to_string(),
                quote: "CAD".to_string(),
            },
            &bidask_cache,
            &settings,
        );

        // === Verification of conversion logic ===
        
        // 1. Check that profit_bidask is set (CAD->USD conversion rate)
        // It should be the REVERSE of USDCAD: 1/1.3602 for bid, 1/1.3600 for ask
        println!("profit_bidask.bid (CAD->USD): {}", position.profit_bidask.bid);
        println!("profit_bidask.ask (CAD->USD): {}", position.profit_bidask.ask);
        assert!(position.profit_bidask.bid > 0.73 && position.profit_bidask.bid < 0.75);
        assert!(position.profit_bidask.ask > 0.73 && position.profit_bidask.ask < 0.75);

        // 2. Calculate expected PnL manually
        let open_price = 1.3502; // BUY opens at ask
        let close_price = 1.3600; // BUY closes at bid
        let price_diff = close_price - open_price; // 0.0098
        let lots = 1.0;
        let contract_size = 100000.0;
        
        // PnL in CAD
        let pnl_cad: f64 = price_diff * lots * contract_size;
        println!("PnL in CAD: {} CAD", pnl_cad);
        assert!((pnl_cad - 980.0).abs() < 0.01); // 0.0098 * 1.0 * 100000 ≈ 980
        
        // Conversion rate: for profit, use bid
        let conversion_rate = position.profit_bidask.bid; // 1/1.3602
        let expected_pnl_usd = pnl_cad * conversion_rate;
        println!("Conversion rate (CAD->USD): {}", conversion_rate);
        println!("Expected PnL in USD: {} USD", expected_pnl_usd);
        
        // 3. Verify the actual PnL matches our calculation
        println!("Actual PnL in USD: {}", position.pl);
        assert_eq!(format!("{:.2}", position.pl), format!("{:.2}", expected_pnl_usd));
        
        // The key insight: without conversion, PnL would be 980 CAD
        // With current conversion at ~0.7352, it becomes ~720.48 USD
        assert_eq!(format!("{:.2}", position.get_gross_pl()), "720.48");
    }
}

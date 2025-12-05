use std::collections::HashSet;

use ahash::AHashSet;
use cross_calculations::core::CrossCalculationsError;

use crate::{
    accounts::{
        account::{MicroEngineAccount, MicroEngineAccountCalculationUpdate},
        account_cache::MicroEngineAccountCache,
    },
    bidask::{MicroEngineBidAskCache, MicroEngineInstrument, dto::MicroEngineBidask},
    positions::{
        position::MicroEnginePosition,
        positions_cache::{MicroEnginePositionCache, MicroEnginePositionCalculationUpdate},
    },
    settings::{MicroEngineTradingGroupSettings, TradingSettingsCache},
};

pub mod accounts;
pub mod bidask;
pub mod main_tests;
pub mod positions;
pub mod settings;


pub fn round_float_to_digits(value: f64, digits: i32) -> f64 {
    let factor = 10_f64.powi(digits);
    (value * factor).round() / factor
}

pub struct MicroEngine {
    accounts: MicroEngineAccountCache,
    positions_cache: MicroEnginePositionCache,
    pub settings_cache: TradingSettingsCache,
    pub bidask_cache: MicroEngineBidAskCache,
    updated_assets: AHashSet<String>,
}
impl MicroEngine {
    pub async fn initialize(
        accounts: Vec<impl Into<MicroEngineAccount>>,
        positions: Vec<impl Into<MicroEnginePosition>>,
        settings: Vec<impl Into<MicroEngineTradingGroupSettings>>,
        collaterals: HashSet<String>,
        instruments: Vec<MicroEngineInstrument>,
        cached_prices: Vec<MicroEngineBidask>,
    ) -> (Self, Vec<CrossCalculationsError>) {
        let accounts_cache = MicroEngineAccountCache::new(accounts);
        let (bidask_cache, bidask_errors) =
            MicroEngineBidAskCache::new(collaterals, instruments, cached_prices);

        let mut cache = Self {
            positions_cache: MicroEnginePositionCache::new(&bidask_cache, positions),
            settings_cache: TradingSettingsCache::new(settings, &accounts_cache),
            accounts: accounts_cache,
            bidask_cache: bidask_cache,
            updated_assets: AHashSet::new(),
        };

        cache.recalculate_all().await;

        (cache, bidask_errors)
    }

    pub async fn handle_new_price(&mut self, new_bidask: Vec<MicroEngineBidask>) {
        for bidask in new_bidask {
            if !self.updated_assets.contains(&bidask.id) {
                self.updated_assets.insert(bidask.id.clone());
            }

            self.bidask_cache.handle_new(&bidask);
        }
    }

    pub async fn trading_settings_changed(
        &mut self,
        settings: impl Into<MicroEngineTradingGroupSettings>,
    ) {
        let settings = settings.into();

        self.settings_cache
            .insert_or_replace_settings(settings.clone());
    }

    pub async fn insert_or_update_account(
        &mut self,
        account: impl Into<MicroEngineAccount>,
    ) -> Result<MicroEngineAccountCalculationUpdate, MicroEngineError> {
        let account: MicroEngineAccount = account.into();

        self.accounts.insert_or_update_account(
            account,
            &mut self.settings_cache,
            &self.positions_cache,
        )
    }

    pub async fn insert_or_update_position(
        &mut self,
        position: impl Into<MicroEnginePosition>,
    ) -> Result<MicroEngineAccountCalculationUpdate, MicroEngineError> {
        let position: MicroEnginePosition = position.into();

        let mut position: MicroEnginePosition = position.into();

        let (_, sources) = self
            .bidask_cache
            .get_price_with_source(&position.quote, &position.collateral)
            .ok_or(MicroEngineError::ProfitPriceNotFond)?;

        position.profit_price_assets_subscriptions = sources.unwrap_or_default();

        // Note: We don't apply markup to open_bidask here because positions from trading engine
        // already have markup applied to open_bidask. We only apply markup to active_bidask
        // when prices update via update_bidask.

        self.positions_cache.add_position(position.clone());

        self.accounts
            .recalculate_account_data(
                &self.settings_cache,
                &self.positions_cache,
                &position.account_id,
            )
            .ok_or(MicroEngineError::AccountNotFound)
    }

    pub async fn remove_position(
        &mut self,
        position_id: &str,
    ) -> Result<MicroEngineAccountCalculationUpdate, MicroEngineError> {
        let removed_position = self
            .positions_cache
            .remove_position(position_id)
            .ok_or(MicroEngineError::PositionNotFound)?;

        self.accounts
            .recalculate_account_data(
                &self.settings_cache,
                &self.positions_cache,
                &removed_position.account_id,
            )
            .ok_or(MicroEngineError::AccountNotFound)
    }

    pub async fn recalculate_accordint_to_updates(
        &mut self,
    ) -> (
        Option<Vec<MicroEngineAccountCalculationUpdate>>,
        Option<Vec<MicroEnginePositionCalculationUpdate>>,
    ) {
        let updated_prices: Vec<String> = {
            if self.updated_assets.is_empty() {
                return (None, None);
            }

            self.updated_assets.drain().collect()
        };

        let positions_update_result = self.positions_cache.recalculate_positions_pl(
            &updated_prices,
            &mut self.bidask_cache,
            &self.settings_cache,
        );

        let Some(positions_update_result) = positions_update_result else {
            return (None, None);
        };

        let updated_accounts = positions_update_result
            .iter()
            .map(|x| x.account_id.as_str())
            .collect::<Vec<_>>();

        let accounts_update_result = self.accounts.recalculate_accounts_data(
            &self.settings_cache,
            &self.positions_cache,
            updated_accounts.as_slice(),
        );

        (Some(accounts_update_result), Some(positions_update_result))
    }

    async fn recalculate_all(&mut self) {
        self.positions_cache
            .recalculate_all_positions(&mut self.bidask_cache, &self.settings_cache);

        self.accounts
            .recalculate_all_accounts(&self.settings_cache, &self.positions_cache);
    }

    pub async fn query_account_cache(
        &self,
        call: impl Fn(&MicroEngineAccountCache) -> Vec<MicroEngineAccount>,
    ) -> Vec<MicroEngineAccount> {
        call(&self.accounts)
    }

    pub async fn query_positions_cache(
        &self,
        call: impl Fn(&MicroEnginePositionCache) -> Vec<MicroEnginePosition>,
    ) -> Vec<MicroEnginePosition> {
        call(&self.positions_cache)
    }
}

#[derive(Debug)]
pub enum MicroEngineError {
    ProfitPriceNotFond,
    AccountNotFound,
    PositionNotFound,
    AccountSettingsNotFound(String),
}

#[cfg(test)]
mod tests {
    use tokio::runtime::Builder;

    use super::*;
    use crate::settings::{TradingGroupInstrumentMarkupSettings, TradingGroupInstrumentSettings};
    use std::collections::{HashMap, HashSet};

    fn sample_settings() -> MicroEngineTradingGroupSettings {
        let mut instruments = HashMap::new();
        instruments.insert(
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
        );

        MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            hedge_coef: None,
            instruments,
        }
    }

    fn sample_settings_with_markup() -> MicroEngineTradingGroupSettings {
        let mut instruments = HashMap::new();
        instruments.insert(
            "EURUSD".to_string(),
            TradingGroupInstrumentSettings {
                digits: 5,
                max_leverage: None,
                markup_settings: Some(TradingGroupInstrumentMarkupSettings {
                    markup_bid: -300.0 * 0.00001,
                    markup_ask: 500.0 * 0.00001,
                    min_spread: None,
                    max_spread: None,
                }),
            },
        );

        MicroEngineTradingGroupSettings {
            id: "tg1".to_string(),
            hedge_coef: None,
            instruments,
        }
    }

    fn sample_account() -> MicroEngineAccount {
        MicroEngineAccount {
            id: "ACC1".to_string(),
            trader_id: "TR1".to_string(),
            trading_group: "tg1".to_string(),
            balance: 100000.0,
            leverage: 100.0,
            margin: 0.0,
            equity: 0.0,
            free_margin: 0.0,
            margin_level: 0.0,
        }
    }

    fn sample_instrument() -> MicroEngineInstrument {
        MicroEngineInstrument {
            id: "EURUSD".to_string(),
            base: "EUR".to_string(),
            quote: "USD".to_string(),
        }
    }

    fn sample_bidask() -> MicroEngineBidask {
        MicroEngineBidask {
            id: "EURUSD".to_string(),
            bid: 1.0,
            ask: 1.1,
            base: "EUR".to_string(),
            quote: "USD".to_string(),
        }
    }

    #[tokio::test]
    async fn test_recalculations_with_min_spread() {
        let account = sample_account();
        let settings = sample_settings();
        let instrument = sample_instrument();

        let collaterals = HashSet::from(["USD".to_string()]);

        let (mut engine, errors) = MicroEngine::initialize(
            vec![account.clone()],
            vec![MicroEnginePosition {
                id: "id".to_string(),
                trader_id: "TR1".to_string(),
                account_id: "ACC1".to_string(),
                base: "EUR".to_string(),
                quote: "USD".to_string(),
                collateral: "USD".to_string(),
                asset_pair: "EURUSD".to_string(),
                lots_amount: 0.05,
                contract_size: 100000.0,
                is_buy: true,
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
            }],
            vec![settings],
            collaterals,
            vec![instrument],
            vec![MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }],
        )
        .await;

        engine
            .handle_new_price(vec![MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }])
            .await;

        let (account_update, _) = engine.recalculate_accordint_to_updates().await;

        let account_update = account_update.unwrap().first().cloned().unwrap();

        assert_eq!(format!("{:.5}", account_update.total_gross), "-0.60000");
        assert_eq!(format!("{:.5}", account_update.margin), "62.77100");
        assert_eq!(format!("{:.5}", account_update.equity), "99999.40000");
        assert_eq!(format!("{:.5}", account_update.free_margin), "99936.62900");

        assert!(errors.is_empty());
    }

    #[tokio::test]
    async fn test_recalculations_with_min_spread2() {
        let account = sample_account();
        let settings = sample_settings();
        let instrument = sample_instrument();

        let collaterals = HashSet::from(["USD".to_string()]);

        let (mut engine, errors) = MicroEngine::initialize(
            vec![account.clone()],
            vec![MicroEnginePosition {
                id: "id".to_string(),
                trader_id: "TR1".to_string(),
                account_id: "ACC1".to_string(),
                base: "EUR".to_string(),
                quote: "USD".to_string(),
                collateral: "USD".to_string(),
                asset_pair: "EURUSD".to_string(),
                lots_amount: 0.01,
                contract_size: 100000.0,
                is_buy: false,
                pl: 0.0,
                commission: 0.0,
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
            }],
            vec![settings],
            collaterals,
            vec![instrument],
            vec![MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }],
        )
        .await;

        engine
            .handle_new_price(vec![MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }])
            .await;

        let (account_update, _) = engine.recalculate_accordint_to_updates().await;

        let account_update = account_update.unwrap().first().cloned().unwrap();

        assert_eq!(format!("{:.5}", account_update.total_gross), "-0.11000");
        assert_eq!(format!("{:.5}", account_update.margin), "12.55400");
        assert_eq!(format!("{:.5}", account_update.equity), "99999.89000");
        assert_eq!(format!("{:.5}", account_update.free_margin), "99987.33600");

        assert!(errors.is_empty());
    }

    #[tokio::test]
    async fn test_recalculations_with_markup() {
        let account = sample_account();
        let settings = sample_settings_with_markup();
        let instrument = sample_instrument();

        let collaterals = HashSet::from(["USD".to_string()]);

        let (mut engine, errors) = MicroEngine::initialize(
            vec![account.clone()],
            vec![MicroEnginePosition {
                id: "id".to_string(),
                trader_id: "TR1".to_string(),
                account_id: "ACC1".to_string(),
                base: "EUR".to_string(),
                quote: "USD".to_string(),
                collateral: "USD".to_string(),
                asset_pair: "EURUSD".to_string(),
                lots_amount: 0.05,
                contract_size: 100000.0,
                is_buy: true,
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
            }],
            vec![settings],
            collaterals,
            vec![instrument],
            vec![MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }],
        )
        .await;

        engine
            .handle_new_price(vec![MicroEngineBidask {
                id: "EURUSD".to_string(),
                bid: 1.25540,
                ask: 1.25542,
                base: "EUR".to_string(),
                quote: "USD".to_string(),
            }])
            .await;

        let (account_update, _) = engine.recalculate_accordint_to_updates().await;

        let account_update = account_update.unwrap().first().cloned().unwrap();

        assert_eq!(format!("{:.5}", account_update.total_gross), "-15.15000");
        assert_eq!(format!("{:.5}", account_update.margin), "62.77100");
        assert_eq!(format!("{:.5}", account_update.equity), "99984.85000");
        assert_eq!(format!("{:.5}", account_update.free_margin), "99922.07900");

        assert!(errors.is_empty());
    }
}

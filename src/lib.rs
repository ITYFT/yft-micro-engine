use std::collections::HashSet;

use ahash::AHashSet;
use cross_calculations::core::CrossCalculationsError;
use tokio::sync::{Mutex, RwLock};

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

pub struct MicroEngine {
    accounts: RwLock<MicroEngineAccountCache>,
    positions_cache: RwLock<MicroEnginePositionCache>,
    settings_cache: RwLock<TradingSettingsCache>,
    pub bidask_cache: RwLock<MicroEngineBidAskCache>,
    updated_assets: Mutex<AHashSet<String>>,
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

        let cache = Self {
            positions_cache: RwLock::new(MicroEnginePositionCache::new(&bidask_cache, positions)),
            settings_cache: RwLock::new(TradingSettingsCache::new(settings, &accounts_cache)),
            accounts: RwLock::new(accounts_cache),
            bidask_cache: RwLock::new(bidask_cache),
            updated_assets: Mutex::new(AHashSet::new()),
        };

        cache.recalculate_all().await;

        (cache, bidask_errors)
    }

    pub async fn handle_new_price(&self, new_bidask: Vec<MicroEngineBidask>) {
        let mut updated_assets = self.updated_assets.lock().await;
        let mut price_cache = self.bidask_cache.write().await;

        for bidask in new_bidask {
            if !updated_assets.contains(&bidask.id) {
                updated_assets.insert(bidask.id.clone());
            }

            price_cache.handle_new(&bidask);
        }
    }

    pub async fn trading_settings_changed(
        &self,
        settings: impl Into<MicroEngineTradingGroupSettings>,
    ) {
        let settings = settings.into();

        let mut settings_cache = self.settings_cache.write().await;
        settings_cache.insert_or_replace_settings(settings.clone());
    }

    pub async fn insert_or_update_account(
        &self,
        account: impl Into<MicroEngineAccount>,
    ) -> Result<MicroEngineAccountCalculationUpdate, MicroEngineError> {
        let account: MicroEngineAccount = account.into();
        let (mut accounts, positions_cache, mut settings_cache) = tokio::join!(
            self.accounts.write(),
            self.positions_cache.read(),
            self.settings_cache.write(),
        );

        accounts.insert_or_update_account(account, &mut settings_cache, &positions_cache)
    }

    pub async fn insert_or_update_position(
        &self,
        position: impl Into<MicroEnginePosition>,
    ) -> Result<MicroEngineAccountCalculationUpdate, MicroEngineError> {
        let position: MicroEnginePosition = position.into();
        let (mut accounts, mut positions_cache, settings_cache, bidask_cache) = tokio::join!(
            self.accounts.write(),
            self.positions_cache.write(),
            self.settings_cache.read(),
            self.bidask_cache.read(),
        );

        let mut position: MicroEnginePosition = position.into();

        let (_, sources) = bidask_cache
            .get_price_with_source(&position.quote, &position.collateral)
            .ok_or(MicroEngineError::ProfitPriceNotFond)?;

        position.profit_price_assets_subscriptions = sources.unwrap_or_default();

        positions_cache.add_position(position.clone());

        accounts
            .recalculate_account_data(&settings_cache, &positions_cache, &position.account_id)
            .ok_or(MicroEngineError::AccountNotFound)
    }

    pub async fn remove_position(
        &self,
        position_id: &str,
    ) -> Result<MicroEngineAccountCalculationUpdate, MicroEngineError> {
        let (mut accounts, mut positions_cache, settings_cache) = tokio::join!(
            self.accounts.write(),
            self.positions_cache.write(),
            self.settings_cache.read(),
        );

        let removed_position = positions_cache
            .remove_position(position_id)
            .ok_or(MicroEngineError::PositionNotFound)?;

        accounts
            .recalculate_account_data(
                &settings_cache,
                &positions_cache,
                &removed_position.account_id,
            )
            .ok_or(MicroEngineError::AccountNotFound)
    }

    pub async fn recalculate_accordint_to_updates(
        &self,
    ) -> (
        Option<Vec<MicroEngineAccountCalculationUpdate>>,
        Option<Vec<MicroEnginePositionCalculationUpdate>>,
    ) {
        // Lock `updated_assets` separately to ensure consistent lock ordering.
        // This matches `handle_new_price`, which locks `updated_assets` before
        // `bidask_cache`.
        let updated_prices: Vec<String> = {
            let mut updated_assets = self.updated_assets.lock().await;

            if updated_assets.is_empty() {
                return (None, None);
            }

            updated_assets.drain().collect()
        };

        let (mut accounts, mut positions_cache, settings_cache, bidask_cache) = tokio::join!(
            self.accounts.write(),
            self.positions_cache.write(),
            self.settings_cache.read(),
            self.bidask_cache.read(),
        );

        let positions_update_result = positions_cache.recalculate_positions_pl(
            &updated_prices,
            &bidask_cache,
            &settings_cache,
        );

        let Some(positions_update_result) = positions_update_result else {
            return (None, None);
        };

        let updated_accounts = positions_update_result
            .iter()
            .map(|x| x.account_id.as_str())
            .collect::<Vec<_>>();

        let accounts_update_result = accounts.recalculate_accounts_data(
            &settings_cache,
            &positions_cache,
            updated_accounts.as_slice(),
        );

        (Some(accounts_update_result), Some(positions_update_result))
    }

    async fn recalculate_all(&self) {
        let (mut accounts, mut positions_cache, settings_cache, bidask_cache) = tokio::join!(
            self.accounts.write(),
            self.positions_cache.write(),
            self.settings_cache.read(),
            self.bidask_cache.read(),
        );

        positions_cache.recalculate_all_positions(&bidask_cache, &settings_cache);

        accounts.recalculate_all_accounts(&settings_cache, &positions_cache);
    }

    pub async fn query_account_cache(
        &self,
        call: impl Fn(&MicroEngineAccountCache) -> Vec<MicroEngineAccount>,
    ) -> Vec<MicroEngineAccount> {
        let accounts = self.accounts.read().await;

        call(&accounts)
    }

    pub async fn query_positions_cache(
        &self,
        call: impl Fn(&MicroEnginePositionCache) -> Vec<MicroEnginePosition>,
    ) -> Vec<MicroEnginePosition> {
        let positions = self.positions_cache.read().await;

        call(&positions)
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
    use crate::settings::TradingGroupInstrumentSettings;
    use std::collections::{HashMap, HashSet};

    fn sample_settings() -> MicroEngineTradingGroupSettings {
        let mut instruments = HashMap::new();
        instruments.insert(
            "EURUSD".to_string(),
            TradingGroupInstrumentSettings {
                digits: 5,
                max_leverage: None,
                markup_settings: None,
            },
        );

        MicroEngineTradingGroupSettings {
            id: "G1".to_string(),
            hedge_coef: None,
            instruments,
        }
    }

    fn sample_account() -> MicroEngineAccount {
        MicroEngineAccount {
            id: "ACC1".to_string(),
            trader_id: "TR1".to_string(),
            trading_group: "G1".to_string(),
            balance: 1000.0,
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
    async fn test_initialize_and_query() {
        let rt = Builder::new_current_thread().enable_all().build().unwrap();

        let account = sample_account();
        let settings = sample_settings();
        let instrument = sample_instrument();
        let bidask = sample_bidask();

        let collaterals = HashSet::from(["USD".to_string()]);

        let (engine, errors) = rt.block_on(MicroEngine::initialize(
            vec![account.clone()],
            Vec::<MicroEnginePosition>::new(),
            vec![settings],
            collaterals,
            vec![instrument],
            vec![bidask],
        ));
        assert!(errors.is_empty());

        let accounts = engine
            .query_account_cache(|cache| cache.get_all_accounts().into_iter().cloned().collect())
            .await;
        assert_eq!(accounts.len(), 1);
        assert_eq!(accounts[0].id, account.id);
    }

    fn sample_position() -> MicroEnginePosition {
        let price = sample_bidask();
        MicroEnginePosition {
            id: "POS1".to_string(),
            trader_id: "TR1".to_string(),
            account_id: "ACC1".to_string(),
            base: "EUR".to_string(),
            quote: "USD".to_string(),
            collateral: "USD".to_string(),
            asset_pair: "EURUSD".to_string(),
            lots_amount: 1.0,
            contract_size: 1.0,
            is_buy: true,
            pl: 0.0,
            commission: 0.0,
            open_bidask: price.clone(),
            active_bidask: price.clone(),
            margin_bidask: price.clone(),
            profit_bidask: MicroEngineBidask::create_blank(),
            profit_price_assets_subscriptions: Vec::new(),
            swaps_sum: 0.0,
        }
    }

    #[tokio::test]
    async fn test_price_update_and_recalc() {
        let rt = Builder::new_current_thread().enable_all().build().unwrap();

        let account = sample_account();
        let settings = sample_settings();
        let instrument = sample_instrument();
        let bidask = sample_bidask();

        let collaterals = HashSet::from(["USD".to_string()]);

        let (engine, errors) = rt.block_on(MicroEngine::initialize(
            vec![account.clone()],
            Vec::<MicroEnginePosition>::new(),
            vec![settings],
            collaterals,
            vec![instrument],
            vec![bidask],
        ));
        assert!(errors.is_empty());

        // insert a position
        let position = sample_position();
        let insert_res = engine.insert_or_update_position(position).await;
        assert!(insert_res.is_ok());

        // update price
        let new_price = MicroEngineBidask {
            id: "EURUSD".to_string(),
            bid: 1.2,
            ask: 1.3,
            base: "EUR".to_string(),
            quote: "USD".to_string(),
        };
        engine.handle_new_price(vec![new_price]).await;

        let (acc_updates, pos_updates) = engine.recalculate_accordint_to_updates().await;
        let (acc_updates, pos_updates) = (acc_updates.unwrap(), pos_updates.unwrap());
        assert_eq!(acc_updates.len(), 1);
        assert_eq!(pos_updates.len(), 1);
        assert_eq!(pos_updates[0].position_id, "POS1");

        // ensure updates cleared
        let (acc_updates2, pos_updates2) = engine.recalculate_accordint_to_updates().await;
        assert!(acc_updates2.is_none());
        assert!(pos_updates2.is_none());
    }
}

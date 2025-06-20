use std::collections::HashSet;

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
    settings::{TradingGroupSettings, TradingSettingsCache},
};

pub mod accounts;
pub mod bidask;
pub mod positions;
pub mod settings;

pub struct MicroEngine {
    accounts: RwLock<MicroEngineAccountCache>,
    positions_cache: RwLock<MicroEnginePositionCache>,
    settings_cache: RwLock<TradingSettingsCache>,
    bidask_cache: RwLock<MicroEngineBidAskCache>,
    updated_assets: Mutex<HashSet<String>>,
}
impl MicroEngine {
    pub fn initialize(
        accounts: Vec<impl Into<MicroEngineAccount>>,
        positions: Vec<impl Into<MicroEnginePosition>>,
        settings: Vec<impl Into<TradingGroupSettings>>,
        collaterals: HashSet<String>,
        instruments: HashSet<MicroEngineInstrument>,
        cached_prices: Vec<MicroEngineBidask>,
    ) -> (Self, Vec<CrossCalculationsError>) {
        let accounts_cache = MicroEngineAccountCache::new(accounts);
        let (bidask_cache, bidask_errors) =
            MicroEngineBidAskCache::new(collaterals, instruments, cached_prices);
        (
            Self {
                positions_cache: RwLock::new(MicroEnginePositionCache::new(positions)),
                settings_cache: RwLock::new(TradingSettingsCache::new(settings, &accounts_cache)),
                accounts: RwLock::new(accounts_cache),
                bidask_cache: RwLock::new(bidask_cache),
                updated_assets: Mutex::new(HashSet::new()),
            },
            bidask_errors,
        )
    }

    pub async fn handle_new_price(&self, new_bidask: Vec<impl Into<MicroEngineBidask>>) {
        let mut updated_assets = self.updated_assets.lock().await;
        let mut price_cache = self.bidask_cache.write().await;

        for bidask in new_bidask {
            let bidask: MicroEngineBidask = bidask.into();
            if !updated_assets.contains(&bidask.id) {
                updated_assets.insert(bidask.id.clone());
            }

            price_cache.handle_new(&bidask);
        }
    }

    pub async fn recalculate_accordint_to_updates(
        &self,
    ) -> (
        Vec<MicroEngineAccountCalculationUpdate>,
        Vec<MicroEnginePositionCalculationUpdate>,
    ) {
        let (mut accounts, mut positions_cache, settings_cache, bidask_cache, mut updated_assets) = tokio::join!(
            self.accounts.write(),
            self.positions_cache.write(),
            self.settings_cache.read(),
            self.bidask_cache.read(),
            self.updated_assets.lock()
        );

        let updated_prices: Vec<String> = updated_assets.drain().collect();

        let positions_update_result = positions_cache.recalculate_positions_pl(
            &updated_prices,
            &bidask_cache,
            &settings_cache,
        );

        let updated_accounts = positions_update_result
            .iter()
            .map(|x| x.position_id.as_str())
            .collect::<Vec<_>>();

        let accounts_update_result = accounts.recalculate_accounts_data(
            &settings_cache,
            &positions_cache,
            updated_accounts.as_slice(),
        );

        (accounts_update_result, positions_update_result)
    }
}

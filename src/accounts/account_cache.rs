use std::collections::HashMap;

use crate::{
    accounts::account::{MicroEngineAccount, MicroEngineAccountCalculationUpdate},
    positions::positions_cache::MicroEnginePositionCache,
    settings::TradingSettingsCache,
};

pub struct MicroEngineAccountCache {
    trader_index: HashMap<String, Vec<String>>,
    accounts: HashMap<String, MicroEngineAccount>,
}

impl MicroEngineAccountCache {
    pub (crate) fn new(accounts: Vec<impl Into<MicroEngineAccount>>) -> Self {
        let mut trader_index: HashMap<String, Vec<String>> = HashMap::new();
        let mut accounts_cache = HashMap::new();

        for account in accounts {
            let account: MicroEngineAccount = account.into();
            let trader_id = account.trader_id.clone();

            trader_index
                .entry(trader_id)
                .or_default()
                .push(account.id.clone());

            accounts_cache.insert(account.id.clone(), account);
        }

        Self {
            trader_index,
            accounts: accounts_cache,
        }
    }

    pub fn get_trader_accounts(&self, trader_id: &str) -> Option<Vec<&MicroEngineAccount>> {
        let accounts = self.trader_index.get(trader_id)?;

        Some(
            accounts
                .into_iter()
                .filter_map(|x| self.accounts.get(x))
                .collect(),
        )
    }

    pub fn get_account(&self, account_id: &str) -> Option<&MicroEngineAccount> {
        self.accounts.get(account_id)
    }

    pub fn get_all_accounts(&self) -> Vec<&MicroEngineAccount> {
        self.accounts.values().collect()
    }

    pub(crate) fn recalculate_accounts_data(
        &mut self,
        settings: &TradingSettingsCache,
        positions_cache: &MicroEnginePositionCache,
        updated_accounts: &[&str],
    ) -> Vec<MicroEngineAccountCalculationUpdate> {
        let mut updated_accounts_data = vec![];

        for account_id in updated_accounts {
            let Some(account_settings) = settings.resolve_by_account(&account_id) else {
                continue;
            };

            let Some(account_positions) = positions_cache.get_account_positions(&account_id) else {
                continue;
            };

            if let Some(account) = self.accounts.get_mut(*account_id) {
                updated_accounts_data.push(
                    account
                        .recalculate_account_data(account_positions.as_slice(), account_settings),
                );
            }
        }
        updated_accounts_data
    }
}

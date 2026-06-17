#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use crate::accounts::account::MicroEngineAccount;
    use crate::bidask::MicroEngineInstrument;
    use crate::positions::position::MicroEnginePosition;
    use crate::settings::{CollateralSettings, MicroEngineTradingGroupSettings, TradingGroupInstrumentSettings};
    use crate::MicroEngine;

    fn make_settings(id: &str) -> MicroEngineTradingGroupSettings {
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
            id: id.to_string(),
            hedge_coef: None,
            instruments,
            collaterals: HashMap::from_iter(vec![
                ("USD".to_string(), CollateralSettings { digits: 2 }),
            ]),
        }
    }

    fn make_account(id: &str, group: &str) -> MicroEngineAccount {
        MicroEngineAccount {
            id: id.to_string(),
            trader_id: "TR1".to_string(),
            trading_group: group.to_string(),
            balance: 1000.0,
            leverage: 100.0,
            margin: 0.0,
            equity: 0.0,
            free_margin: 0.0,
            margin_level: 0.0,
        }
    }

    async fn make_engine(accounts: Vec<MicroEngineAccount>, groups: Vec<MicroEngineTradingGroupSettings>) -> MicroEngine {
        let collaterals = HashSet::from(["USD".to_string()]);
        let instrument = MicroEngineInstrument {
            id: "EURUSD".to_string(),
            base: "EUR".to_string(),
            quote: "USD".to_string(),
        };
        let (engine, errors) = MicroEngine::initialize(
            accounts,
            Vec::<MicroEnginePosition>::new(),
            groups,
            collaterals,
            vec![instrument],
            vec![],
        )
        .await;
        assert!(errors.is_empty());
        engine
    }

    #[tokio::test]
    async fn test_remove_trading_group_removes_from_settings_cache() {
        let mut engine = make_engine(
            vec![make_account("ACC1", "G1")],
            vec![make_settings("G1"), make_settings("G2")],
        )
        .await;

        engine.remove_trading_group("G1");

        assert!(engine.settings_cache.groups.get("G1").is_none());
        assert!(engine.settings_cache.groups.get("G2").is_some());
    }

    #[tokio::test]
    async fn test_remove_trading_group_returns_affected_accounts() {
        let mut engine = make_engine(
            vec![
                make_account("ACC1", "G1"),
                make_account("ACC2", "G1"),
                make_account("ACC3", "G2"),
            ],
            vec![make_settings("G1"), make_settings("G2")],
        )
        .await;

        let mut affected = engine.remove_trading_group("G1");
        affected.sort();

        assert_eq!(affected, vec!["ACC1".to_string(), "ACC2".to_string()]);
    }

    #[tokio::test]
    async fn test_remove_nonexistent_group_is_noop() {
        let mut engine = make_engine(
            vec![make_account("ACC1", "G1")],
            vec![make_settings("G1")],
        )
        .await;

        let affected = engine.remove_trading_group("NONEXISTENT");

        assert!(affected.is_empty());
        assert!(engine.settings_cache.groups.get("G1").is_some());
    }
}

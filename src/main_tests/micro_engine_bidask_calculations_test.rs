// Test for MicroEngine entity: setup, price update, and recalc

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::account::MicroEngineAccount;
    use crate::bidask::MicroEngineInstrument;
    use crate::bidask::dto::MicroEngineBidask;
    use crate::positions::position::MicroEnginePosition;
    use crate::settings::TradingGroupInstrumentSettings;
    use crate::{MicroEngine, settings::MicroEngineTradingGroupSettings};
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
    async fn test_micro_engine_handle_new_price() {
        let account = sample_account();
        let settings = sample_settings();
        let instrument = sample_instrument();
        let bidask = sample_bidask();

        let collaterals = HashSet::from(["USD".to_string()]);
        let (mut engine, errors) = MicroEngine::initialize(
            vec![account.clone()],
            Vec::<MicroEnginePosition>::new(),
            vec![settings],
            collaterals,
            vec![instrument],
            vec![bidask],
        )
        .await;
        assert!(errors.is_empty());
        // Insert a position
        let position = sample_position();
        let insert_res = engine.insert_or_update_position(position).await;
        assert!(insert_res.is_ok());
        // Update price
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
    }
}

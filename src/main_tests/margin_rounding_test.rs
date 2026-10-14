//! SKILLTRADE-537: the live calculation must report margin, free margin and margin level exactly
//! as the trading engine does, because the engine is authoritative (margin calls, stop-outs).

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use crate::MicroEngine;
    use crate::accounts::account::{MicroEngineAccount, MicroEngineAccountCalculationUpdate};
    use crate::bidask::MicroEngineInstrument;
    use crate::bidask::dto::MicroEngineBidask;
    use crate::positions::position::MicroEnginePosition;
    use crate::settings::{
        CollateralSettings, MicroEngineTradingGroupSettings, TradingGroupInstrumentSettings,
    };

    /// A verbatim copy of the trading engine's arithmetic, used as the reference.
    ///
    /// Source: ITYFT/trading-engine-new, trading-engine-core/src/positions/cache/account_statistic.rs
    /// (`AccountTradingStatisticByAssetPair::calculate_margin`, `AccountTradingStatistic::
    /// {calculate_margin, position_added, get_equity, get_free_margin, get_margin_lvl}`) and
    /// trading-engine-core/src/lib.rs (`round_float_to_digits`). Identical at tags 0.1.175, 0.2.0
    /// and master 38fbe40. Only the data shapes are adapted; the operations are in the same order.
    mod engine_reference {
        pub struct Leg {
            pub lots: f64,
            pub margin_price: f64,
            pub is_buy: bool,
            pub gross_pl: f64,
        }

        pub struct Instrument {
            pub legs: Vec<Leg>,
            pub lot_size: f64,
            pub max_leverage: Option<f64>,
        }

        pub fn round_float_to_digits(value: f64, digits: i32) -> f64 {
            let factor = 10_f64.powi(digits as i32);
            (value * factor).round() / factor
        }

        fn asset_pair_margin(
            instrument: &Instrument,
            account_leverage: f64,
            margin_coef: Option<f64>,
        ) -> f64 {
            // position_created(): running sums per side
            let mut buy_lots_on_margin_price = 0.0;
            let mut sell_lots_on_margin_price = 0.0;
            let mut buy_lots = 0.0;
            let mut sell_lots = 0.0;

            for leg in &instrument.legs {
                if leg.is_buy {
                    buy_lots_on_margin_price += leg.lots * leg.margin_price;
                    buy_lots += leg.lots;
                } else {
                    sell_lots_on_margin_price += leg.lots * leg.margin_price;
                    sell_lots += leg.lots;
                }
            }

            if instrument.legs.is_empty() {
                return 0.0;
            }

            let leverage = match instrument.max_leverage {
                Some(x) => x.min(account_leverage),
                None => account_leverage,
            };

            let hedge_volume = f64::min(buy_lots, sell_lots);

            let hedged_margin = {
                if buy_lots > 0.0 && sell_lots > 0.0 {
                    let margin_coef = margin_coef.unwrap_or(1.0);

                    let hedge_margin_price = (buy_lots_on_margin_price + sell_lots_on_margin_price)
                        / (buy_lots + sell_lots);

                    hedge_volume * instrument.lot_size * hedge_margin_price / leverage * margin_coef
                } else {
                    0.0
                }
            };

            let not_hedged_margin_price = match buy_lots > sell_lots {
                true => buy_lots_on_margin_price / buy_lots,
                false => sell_lots_on_margin_price / sell_lots,
            };

            let not_hedged_lots = (buy_lots - sell_lots).abs();

            let not_hedge_margin =
                not_hedged_lots * instrument.lot_size * not_hedged_margin_price / leverage;

            hedged_margin + not_hedge_margin
        }

        /// (margin, equity, free_margin, margin_level) as the engine reports them.
        pub fn account(
            balance: f64,
            account_leverage: f64,
            collateral_digits: i32,
            hedge_coefficient: Option<f64>,
            instruments: &[Instrument],
        ) -> (f64, f64, f64, f64) {
            let mut margin = 0.0;
            for instrument in instruments {
                margin += asset_pair_margin(instrument, account_leverage, hedge_coefficient);
            }
            let margin = round_float_to_digits(margin, collateral_digits);

            let gross: f64 = instruments
                .iter()
                .map(|instrument| {
                    let buy_sum = instrument
                        .legs
                        .iter()
                        .filter(|leg| leg.is_buy)
                        .map(|leg| leg.gross_pl)
                        .sum::<f64>();
                    let sell_sum = instrument
                        .legs
                        .iter()
                        .filter(|leg| !leg.is_buy)
                        .map(|leg| leg.gross_pl)
                        .sum::<f64>();
                    buy_sum + sell_sum
                })
                .sum();

            let equity = balance + gross;
            let free_margin = equity - margin;
            let has_positions = instruments.iter().any(|instrument| !instrument.legs.is_empty());
            let margin_level = if !has_positions {
                0.0
            } else {
                round_float_to_digits(equity / margin * 100.0, 2)
            };

            (margin, equity, free_margin, margin_level)
        }
    }

    use engine_reference::{Instrument, Leg};

    const GROUP: &str = "G1";

    fn settings(
        instruments: &[(&str, Option<f64>)],
        hedge_coef: Option<f64>,
        collateral: &str,
        collateral_digits: u32,
    ) -> MicroEngineTradingGroupSettings {
        MicroEngineTradingGroupSettings {
            id: GROUP.to_string(),
            hedge_coef,
            instruments: instruments
                .iter()
                .map(|(id, max_leverage)| {
                    (
                        id.to_string(),
                        TradingGroupInstrumentSettings {
                            digits: 2,
                            max_leverage: *max_leverage,
                            markup_settings: None,
                        },
                    )
                })
                .collect(),
            collaterals: HashMap::from([(
                collateral.to_string(),
                CollateralSettings {
                    digits: collateral_digits,
                },
            )]),
        }
    }

    fn account(balance: f64, leverage: f64) -> MicroEngineAccount {
        MicroEngineAccount {
            id: "ACC1".to_string(),
            trader_id: "TR1".to_string(),
            trading_group: GROUP.to_string(),
            balance,
            leverage,
            margin: 0.0,
            equity: 0.0,
            free_margin: 0.0,
            margin_level: 0.0,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn position(
        id: &str,
        asset_pair: &str,
        collateral: &str,
        is_buy: bool,
        lots: f64,
        contract_size: f64,
        margin_price: f64,
        pl: f64,
    ) -> MicroEnginePosition {
        // The margin price is the open side of the margin bid/ask: ask for a buy, bid for a sell.
        let margin_bidask = MicroEngineBidask {
            id: asset_pair.to_string(),
            bid: margin_price,
            ask: margin_price,
            base: asset_pair[..3].to_string(),
            quote: asset_pair[3..].to_string(),
        };

        MicroEnginePosition {
            id: id.to_string(),
            trader_id: "TR1".to_string(),
            account_id: "ACC1".to_string(),
            base: asset_pair[..3].to_string(),
            quote: asset_pair[3..].to_string(),
            collateral: collateral.to_string(),
            asset_pair: asset_pair.to_string(),
            lots_amount: lots,
            contract_size,
            is_buy,
            pl,
            margin_bidask: margin_bidask.clone(),
            open_bidask: margin_bidask.clone(),
            active_bidask: margin_bidask.clone(),
            profit_bidask: MicroEngineBidask {
                bid: 1.0,
                ask: 1.0,
                ..margin_bidask
            },
            ..Default::default()
        }
    }

    fn micro(
        mut account: MicroEngineAccount,
        positions: &[MicroEnginePosition],
        settings: &MicroEngineTradingGroupSettings,
    ) -> MicroEngineAccountCalculationUpdate {
        let positions: Vec<&MicroEnginePosition> = positions.iter().collect();
        account.recalculate_account_data(&positions, settings)
    }

    /// The SKILLTRADE-537 measurement: AQAUSD, 0.1 Buy (margin price 1000.01) and 0.1 Sell
    /// (1100.00), contract size 1, leverage 100, hedge coefficient 0.5, USD with 2 digits.
    fn ticket_hedged_pair(gross_pl_each: f64) -> Vec<MicroEnginePosition> {
        vec![
            position("P1", "AQAUSD", "USD", true, 0.1, 1.0, 1000.01, gross_pl_each),
            position("P2", "AQAUSD", "USD", false, 0.1, 1.0, 1100.00, gross_pl_each),
        ]
    }

    #[test]
    fn the_tickets_hedged_pair_reports_the_engines_0_53_not_0_525() {
        let settings = settings(&[("AQAUSD", None)], Some(0.5), "USD", 2);

        // Equity 10009.0, the state in which the accounts channel showed the engine's numbers.
        let update = micro(account(10000.0, 100.0), &ticket_hedged_pair(4.5), &settings);

        assert_eq!(update.margin, 0.53, "was 0.5250024999999999 before the fix");
        assert_eq!(update.equity, 10009.0);
        assert_eq!(update.free_margin, 10008.47);
        assert_eq!(update.margin_level, 1888490.57);
    }

    #[test]
    fn the_tickets_single_buy_reports_the_engines_rounded_margin() {
        // 0.05 lot AQAUSD Buy, margin price 1000.02, leverage 100: 0.5000100000000001 unrounded.
        let settings = settings(&[("AQAUSD", None)], None, "USD", 2);
        let positions = [position("P1", "AQAUSD", "USD", true, 0.05, 1.0, 1000.02, -0.75)];

        let update = micro(account(10000.0, 100.0), &positions, &settings);

        assert_eq!(update.margin, 0.5);
        assert_eq!(update.free_margin, 9999.25 - 0.5);
        assert_eq!(update.margin_level, 1999850.0);
    }

    #[test]
    fn an_account_without_positions_has_zero_margin_and_margin_level() {
        let settings = settings(&[("AQAUSD", None)], None, "USD", 2);

        let update = micro(account(10000.0, 100.0), &[], &settings);

        assert_eq!(update.margin, 0.0);
        assert_eq!(update.free_margin, 10000.0);
        assert_eq!(update.margin_level, 0.0);
    }

    #[test]
    fn the_margin_is_rounded_to_the_collaterals_own_digits() {
        // 0.0123 lot * 100000 * 1.23456 / 100 = 15.1850880...
        let positions = [position("P1", "EURUSD", "XYZ", true, 0.0123, 100000.0, 1.23456, 0.0)];

        let three = micro(
            account(1000.0, 100.0),
            &positions,
            &settings(&[("EURUSD", None)], None, "XYZ", 3),
        );
        assert_eq!(three.margin, 15.185);

        let zero = micro(
            account(1000.0, 100.0),
            &positions,
            &settings(&[("EURUSD", None)], None, "XYZ", 0),
        );
        assert_eq!(zero.margin, 15.0);
    }

    #[test]
    fn a_collateral_without_settings_falls_back_to_2_digits() {
        let positions = [position("P1", "EURUSD", "ABC", true, 0.0123, 100000.0, 1.23456, 0.0)];

        let update = micro(
            account(1000.0, 100.0),
            &positions,
            &settings(&[("EURUSD", None)], None, "USD", 5),
        );

        assert_eq!(update.margin, 15.19);
    }

    /// The same fixtures, both ways: the micro engine and the engine's arithmetic must agree on
    /// every value, bit for bit.
    #[test]
    fn the_micro_engine_matches_the_engines_arithmetic_on_every_fixture() {
        struct Fixture {
            name: &'static str,
            balance: f64,
            leverage: f64,
            digits: u32,
            hedge: Option<f64>,
            // (asset pair, max leverage, contract size, legs: (is_buy, lots, margin price, gross pl))
            instruments: Vec<(&'static str, Option<f64>, f64, Vec<(bool, f64, f64, f64)>)>,
        }

        let fixtures = vec![
            Fixture {
                name: "ticket hedged pair",
                balance: 10000.0,
                leverage: 100.0,
                digits: 2,
                hedge: Some(0.5),
                instruments: vec![(
                    "AQAUSD",
                    None,
                    1.0,
                    vec![(true, 0.1, 1000.01, 4.5), (false, 0.1, 1100.0, 4.5)],
                )],
            },
            Fixture {
                name: "ticket single buy",
                balance: 10000.0,
                leverage: 100.0,
                digits: 2,
                hedge: None,
                instruments: vec![("AQAUSD", None, 1.0, vec![(true, 0.05, 1000.02, -0.75)])],
            },
            Fixture {
                name: "partly hedged, max leverage below the account's",
                balance: 5321.17,
                leverage: 500.0,
                digits: 2,
                hedge: Some(0.25),
                instruments: vec![(
                    "EURUSD",
                    Some(30.0),
                    100000.0,
                    vec![
                        (true, 0.37, 1.08431, -12.31),
                        (true, 0.11, 1.08502, 3.07),
                        (false, 0.23, 1.08377, 8.92),
                    ],
                )],
            },
            Fixture {
                name: "two instruments, sells only and buys only",
                balance: 250.0,
                leverage: 1000.0,
                digits: 2,
                hedge: None,
                instruments: vec![
                    ("XAUUSD", None, 100.0, vec![(false, 0.01, 2387.115, -1.25)]),
                    ("GBPUSD", None, 100000.0, vec![(true, 0.07, 1.26995, 0.84)]),
                ],
            },
            Fixture {
                name: "8 digit collateral",
                balance: 0.75,
                leverage: 10.0,
                digits: 8,
                hedge: None,
                instruments: vec![("ETHBTC", None, 1.0, vec![(true, 0.3, 0.0512345, 0.0004)])],
            },
            Fixture {
                name: "no positions",
                balance: 100.0,
                leverage: 100.0,
                digits: 2,
                hedge: None,
                instruments: vec![],
            },
        ];

        for fixture in fixtures {
            let group_settings = settings(
                &fixture
                    .instruments
                    .iter()
                    .map(|(id, max_leverage, _, _)| (*id, *max_leverage))
                    .collect::<Vec<_>>(),
                fixture.hedge,
                "COL",
                fixture.digits,
            );

            let mut positions = vec![];
            let mut reference_instruments = vec![];

            for (asset_pair, max_leverage, contract_size, legs) in &fixture.instruments {
                let mut reference_legs = vec![];
                for (n, (is_buy, lots, margin_price, gross_pl)) in legs.iter().enumerate() {
                    positions.push(position(
                        &format!("{asset_pair}-{n}"),
                        asset_pair,
                        "COL",
                        *is_buy,
                        *lots,
                        *contract_size,
                        *margin_price,
                        *gross_pl,
                    ));
                    reference_legs.push(Leg {
                        lots: *lots,
                        margin_price: *margin_price,
                        is_buy: *is_buy,
                        gross_pl: *gross_pl,
                    });
                }
                reference_instruments.push(Instrument {
                    legs: reference_legs,
                    lot_size: *contract_size,
                    max_leverage: *max_leverage,
                });
            }

            let update = micro(
                account(fixture.balance, fixture.leverage),
                &positions,
                &group_settings,
            );

            let (margin, equity, free_margin, margin_level) = engine_reference::account(
                fixture.balance,
                fixture.leverage,
                fixture.digits as i32,
                fixture.hedge,
                &reference_instruments,
            );

            assert_eq!(update.margin.to_bits(), margin.to_bits(), "{}: margin", fixture.name);
            assert_eq!(update.equity.to_bits(), equity.to_bits(), "{}: equity", fixture.name);
            assert_eq!(
                update.free_margin.to_bits(),
                free_margin.to_bits(),
                "{}: free margin",
                fixture.name
            );
            assert_eq!(
                update.margin_level.to_bits(),
                margin_level.to_bits(),
                "{}: margin level",
                fixture.name
            );
        }
    }

    /// The live path (a position arriving) reports the rounded values too.
    #[tokio::test]
    async fn a_position_update_streams_the_engines_values() {
        let (mut engine, errors) = MicroEngine::initialize(
            vec![account(10000.0, 100.0)],
            Vec::<MicroEnginePosition>::new(),
            vec![settings(&[("AQAUSD", None)], Some(0.5), "USD", 2)],
            HashSet::from(["USD".to_string()]),
            vec![MicroEngineInstrument {
                id: "AQAUSD".to_string(),
                base: "AQA".to_string(),
                quote: "USD".to_string(),
            }],
            vec![MicroEngineBidask {
                id: "AQAUSD".to_string(),
                bid: 1050.0,
                ask: 1050.0,
                base: "AQA".to_string(),
                quote: "USD".to_string(),
            }],
        )
        .await;
        assert!(errors.is_empty());

        let mut last = None;
        for position in ticket_hedged_pair(0.0) {
            last = Some(engine.insert_or_update_position(position).await.unwrap());
        }
        let update = last.unwrap();

        assert_eq!(update.margin, 0.53);
        assert_eq!(update.free_margin, update.equity - 0.53);
        assert_eq!(
            update.margin_level,
            ((update.equity / 0.53 * 100.0) * 100.0).round() / 100.0
        );
    }
}

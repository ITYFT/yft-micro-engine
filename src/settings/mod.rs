use ahash::AHashMap;
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::{Decimal, RoundingStrategy, prelude::FromPrimitive};
use std::collections::HashMap;

use crate::accounts::account::MicroEngineAccount;
use crate::accounts::account_cache::MicroEngineAccountCache;
use crate::bidask::dto::MicroEngineBidask;

#[derive(Debug)]
pub struct TradingSettingsCache {
    accounts_mapping: AHashMap<String, String>,
    groups: AHashMap<String, MicroEngineTradingGroupSettings>,
}

impl TradingSettingsCache {
    pub(crate) fn new(
        settings: Vec<impl Into<MicroEngineTradingGroupSettings>>,
        accounts_cache: &MicroEngineAccountCache,
    ) -> Self {
        let mut groups = AHashMap::new();

        let accounts_mapping = accounts_cache
            .get_all_accounts()
            .into_iter()
            .map(|x| (x.id.clone(), x.trading_group.clone()));

        for group in settings {
            let group: MicroEngineTradingGroupSettings = group.into();

            groups.insert(group.id.clone(), group);
        }

        Self {
            accounts_mapping: accounts_mapping.collect(),
            groups,
        }
    }

    pub fn resolve_by_account(&self, account: &str) -> Option<&MicroEngineTradingGroupSettings> {
        let target_group = self.accounts_mapping.get(account)?;
        self.groups.get(target_group)
    }

    pub fn account_updated(&mut self, account: &MicroEngineAccount) {
        self.accounts_mapping
            .insert(account.id.clone(), account.trading_group.clone());
    }

    pub fn insert_or_replace_settings(
        &mut self,
        settings: MicroEngineTradingGroupSettings,
    ) -> Vec<String> {
        let settings_id = settings.id.clone();
        let mut result = vec![];
        self.groups.insert(settings.id.clone(), settings);

        for (account_id, group_id) in &self.accounts_mapping {
            if group_id == &settings_id {
                result.push(account_id.clone());
            }
        }

        result
    }
}

#[derive(Debug, Clone)]
pub struct MicroEngineTradingGroupSettings {
    pub id: String,
    pub hedge_coef: Option<f64>,
    pub instruments: HashMap<String, TradingGroupInstrumentSettings>,
}

#[derive(Debug, Clone)]
pub struct TradingGroupInstrumentSettings {
    pub digits: u32,
    pub max_leverage: Option<f64>,
    pub markup_settings: Option<TradingGroupInstrumentMarkupSettings>,
}

#[derive(Debug, Clone)]
pub struct TradingGroupInstrumentMarkupSettings {
    pub markup_bid: f64,
    pub markup_ask: f64,
    pub min_spread: Option<f64>,
    pub max_spread: Option<f64>,
}

impl TradingGroupInstrumentSettings {
    pub fn calculate_bidask(&self, bidask: &MicroEngineBidask) -> (f64, f64) {
        let Some(markup_settings) = &self.markup_settings else {
            return (bidask.bid, bidask.ask);
        };

        let (mut bid, mut ask) =
            bidask.get_bid_ask_with_markup(markup_settings.markup_bid, markup_settings.markup_ask);

        if let Some(max_spread) = markup_settings.max_spread {
            (bid, ask) = calculate_max_spread(bid, ask, max_spread, self.digits as u32);
        }

        if let Some(min_spread) = markup_settings.min_spread {
            (bid, ask) = calculate_min_spread(bid, ask, min_spread, self.digits as u32);
        }

        (bid, ask)
    }

    pub fn mutate_bidask(&self, bidask: &mut MicroEngineBidask) {
        if let Some(markup_settings) = &self.markup_settings {
            bidask.apply_markup(markup_settings.markup_bid, markup_settings.markup_ask);

            if let Some(max_spread) = markup_settings.max_spread {
                apply_max_spread(bidask, max_spread, self.digits);
            }

            if let Some(min_spread) = markup_settings.min_spread {
                apply_min_spread(bidask, min_spread, self.digits);
            }
        }
    }
}

fn calculate_max_spread(bid: f64, ask: f64, max_spread: f64, digits: u32) -> (f64, f64) {
    let spread = calculate_spread(bid, ask, digits);
    let max_spread = Decimal::from_f64(max_spread).unwrap();
    let factor = i64::pow(10, digits as u32) as f64;
    let pip = 1.0 / factor;

    if spread > max_spread {
        let spread_diff =
            (spread - max_spread).round_dp_with_strategy(digits, RoundingStrategy::ToZero);

        let spread_rounded = (spread_diff / Decimal::from_f64(2.0).unwrap())
            .round_dp_with_strategy(digits, RoundingStrategy::ToZero);

        let spread_rounded = spread_rounded.to_f64().unwrap();

        let is_odd: bool = (spread_diff * Decimal::from_f64(factor).unwrap())
            .to_i32()
            .unwrap()
            % 2
            == 0;

        if is_odd {
            return (bid + spread_rounded, ask - spread_rounded);
        } else {
            return (bid + spread_rounded + pip, ask - spread_rounded);
        }
    }

    return (bid, ask);
}

fn calculate_min_spread(bid: f64, ask: f64, min_spread: f64, digits: u32) -> (f64, f64) {
    let spread = calculate_spread(bid, ask, digits);
    let min_spread = Decimal::from_f64(min_spread).unwrap();
    let factor = i64::pow(10, digits as u32) as f64;
    let pip = 1.0 / factor;

    if spread < min_spread {
        let spread_diff =
            (min_spread - spread).round_dp_with_strategy(digits, RoundingStrategy::ToZero);
        let spread_rounded = (spread_diff / Decimal::from_f64(2.0).unwrap())
            .round_dp_with_strategy(digits, RoundingStrategy::ToZero);

        let spread_rounded = spread_rounded.to_f64().unwrap();
        let is_odd: bool = (spread_diff * Decimal::from_f64(factor).unwrap())
            .to_i32()
            .unwrap()
            % 2
            == 0;

        let spread_rounded = spread_rounded.to_f64().unwrap();
        if is_odd {
            return (bid - spread_rounded, ask + spread_rounded);
        } else {
            return (bid - spread_rounded + pip, ask + spread_rounded);
        }
    }
    return (bid, ask);
}

fn apply_max_spread(bid_ask: &mut MicroEngineBidask, max_spread: f64, digits: u32) {
    let spread = calculate_spread(bid_ask.bid, bid_ask.ask, digits);
    let max_spread = Decimal::from_f64(max_spread).unwrap();
    let factor = i64::pow(10, digits as u32) as f64;
    let pip = 1.0 / factor;

    if spread > max_spread {
        let spread_diff =
            (spread - max_spread).round_dp_with_strategy(digits, RoundingStrategy::ToZero);

        let spread_rounded = (spread_diff / Decimal::from_f64(2.0).unwrap())
            .round_dp_with_strategy(digits, RoundingStrategy::ToZero);

        let spread_rounded = spread_rounded.to_f64().unwrap();

        let is_odd: bool = (spread_diff * Decimal::from_f64(factor).unwrap())
            .to_i32()
            .unwrap()
            % 2
            == 0;

        if is_odd {
            bid_ask.bid += spread_rounded;
            bid_ask.ask -= spread_rounded;
        } else {
            bid_ask.bid += spread_rounded + pip;
            bid_ask.ask -= spread_rounded;
        }
    }
}

fn apply_min_spread(bid_ask: &mut MicroEngineBidask, min_spread: f64, digits: u32) {
    let spread = calculate_spread(bid_ask.bid, bid_ask.ask, digits);
    let min_spread = Decimal::from_f64(min_spread).unwrap();
    let factor = i64::pow(10, digits as u32) as f64;
    let pip = 1.0 / factor;

    if spread < min_spread {
        let spread_diff =
            (min_spread - spread).round_dp_with_strategy(digits, RoundingStrategy::ToZero);
        let spread_rounded = (spread_diff / Decimal::from_f64(2.0).unwrap())
            .round_dp_with_strategy(digits, RoundingStrategy::ToZero);

        let spread_rounded = spread_rounded.to_f64().unwrap();
        let is_odd: bool = (spread_diff * Decimal::from_f64(factor).unwrap())
            .to_i32()
            .unwrap()
            % 2
            == 0;

        let spread_rounded = spread_rounded.to_f64().unwrap();
        if is_odd {
            bid_ask.bid -= spread_rounded;
            bid_ask.ask += spread_rounded;
        } else {
            bid_ask.bid -= spread_rounded + pip;
            bid_ask.ask += spread_rounded;
        }
    }
}

fn calculate_spread(bid: f64, ask: f64, digits: u32) -> Decimal {
    let bid = Decimal::from_f64(bid).unwrap();
    let ask = Decimal::from_f64(ask).unwrap();
    (ask - bid).round_dp_with_strategy(digits, RoundingStrategy::ToZero)
}

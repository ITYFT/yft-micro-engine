use std::sync::Arc;

use chrono::Utc;
use cross_calculations::core::{CrossCalculationsBidAsk, CrossCalculationsCrossRate};

pub type AStr = Arc<str>;

#[derive(Default, Clone, Debug)]
pub struct MicroEngineBidask {
    pub id: AStr,
    pub bid: f64,
    pub ask: f64,
    pub base: AStr,
    pub quote: AStr,
}

impl CrossCalculationsBidAsk for MicroEngineBidask {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_bid(&self) -> f64 {
        self.bid
    }

    fn get_ask(&self) -> f64 {
        self.ask
    }

    fn get_date(&self) -> chrono::DateTime<chrono::Utc> {
        Utc::now()
    }
}

impl MicroEngineBidask {
    pub fn get_bid_ask_with_markup(&self, markup_bid: f64, markup_ask: f64) -> (f64, f64) {
        let bid = self.bid + markup_bid;
        let ask = self.ask + markup_ask;
        (bid, ask)
    }

    pub fn apply_markup(&mut self, markup_bid: f64, markup_ask: f64) {
        let (bid, ask) = self.get_bid_ask_with_markup(markup_bid, markup_ask);
        self.bid = bid;
        self.ask = ask;
    }

    pub fn get_open_price(&self, is_buy: bool) -> f64 {
        match is_buy {
            true => self.ask,
            false => self.bid,
        }
    }

    pub fn get_close_price(&self, is_buy: bool) -> f64 {
        match is_buy {
            true => self.bid,
            false => self.ask,
        }
    }

    pub fn update_open_price(&mut self, is_buy: bool, price: f64) {
        match is_buy {
            true => self.ask = price,
            false => self.bid = price,
        };
    }

    pub fn update_close_price(&mut self, is_buy: bool, price: f64) {
        match is_buy {
            true => self.bid = price,
            false => self.ask = price,
        };
    }

    pub fn reverse(&self) -> Self {
        let rid = Arc::<str>::from(format!("REVERSE-{}", self.id));
        Self {
            id: rid,
            bid: 1.0 / self.ask,
            ask: 1.0 / self.bid,
            base: self.quote.clone(),
            quote: self.base.clone(),
        }
    }

    pub fn create_blank() -> Self {
        Self {
            id: Arc::<str>::from(""),
            bid: 1.0,
            ask: 1.0,
            base: Arc::<str>::from(""),
            quote:Arc::<str>::from(""),
        }
    }

}

impl From<CrossCalculationsCrossRate> for MicroEngineBidask {
    fn from(value: CrossCalculationsCrossRate) -> Self {
        let id = value.source.map_or(value.base.clone(), |(l, r)| {
            format!("{}-{}", l.get_source(), r.get_source())
        });
        MicroEngineBidask {
            id:   Arc::<str>::from(id),
            bid:  value.bid,
            ask:  value.ask,
            base: Arc::<str>::from(value.base),
            quote:Arc::<str>::from(value.quote),
        }
    }
}


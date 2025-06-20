use chrono::Utc;
use cross_calculations::core::{CrossCalculationsBidAsk, CrossCalculationsCrossRate};

#[derive(Default, Clone, Debug)]
pub struct MicroEngineBidask {
    pub id: String,
    pub bid: f64,
    pub ask: f64,
    pub base: String,
    pub quote: String,
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
        Self {
            id: format!("REVERSE-{}", self.id.clone()),
            bid: 1.0 / self.ask,
            ask: 1.0 / self.bid,
            base: self.quote.clone(),
            quote: self.base.clone(),
        }
    }

    pub fn create_blank() -> Self {
        Self {
            id: String::default(),
            bid: 1.0,
            ask: 1.0,
            base: String::default(),
            quote: String::default(),
        }
    }
}

impl From<CrossCalculationsCrossRate> for MicroEngineBidask {
    fn from(value: CrossCalculationsCrossRate) -> Self {
        Self {
            id: value.source.map_or(value.base.clone(), |(left, right)| {
                format!("{}-{}", left.get_source(), right.get_source())
            }),
            bid: value.bid,
            ask: value.ask,
            base: value.base,
            quote: value.quote,
        }
    }
}

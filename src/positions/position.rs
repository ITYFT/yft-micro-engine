use chrono::{DateTime, Utc};

use crate::{
    bidask::{MicroEngineBidAskCache, dto::MicroEngineBidask},
    settings::MicroEngineTradingGroupSettings,
};

#[derive(Default, Clone, Debug)]
pub struct MicroEnginePositionSwap {
    pub date: DateTime<Utc>,
    pub delta: f64,
}

#[derive(Default, Clone, Debug)]
pub struct MicroEnginePosition {
    pub id: String,
    pub trader_id: String,
    pub account_id: String,
    pub base: String,
    pub quote: String,
    pub collateral: String,
    pub asset_pair: String,
    pub lots_amount: f64,
    pub contract_size: f64,
    pub is_buy: bool,
    pub pl: f64,
    pub commission: f64,
    pub open_bidask: MicroEngineBidask,
    pub active_bidask: MicroEngineBidask,
    pub margin_bidask: MicroEngineBidask,
    pub profit_bidask: MicroEngineBidask,
    pub profit_price_assets_subscriptions: Vec<String>,
    pub swaps_sum: f64,
}

impl MicroEnginePosition {
    pub fn get_gross_pl(&self) -> f64 {
        self.pl - self.commission + self.swaps_sum
    }

    pub fn update_bidask(
        &mut self,
        bidask: &MicroEngineBidask,
        bidask_cache: &MicroEngineBidAskCache,
        settings: &MicroEngineTradingGroupSettings,
    ) {
        let Some(instrument_settings) = settings.instruments.get(&bidask.id) else {
            return;
        };

        let (new_bid, new_ask) = instrument_settings.calculate_bidask(bidask);

        if self.asset_pair == bidask.id {
            self.active_bidask.bid = new_bid;
            self.active_bidask.ask = new_ask;
        }

        let mut profit_hit = false;

        for asset in &self.profit_price_assets_subscriptions {
            if asset == &bidask.id {
                profit_hit = true;
                break;
            }
        }

        if profit_hit {
            if let Some(profit_price) = bidask_cache.get_price(&self.quote, &self.collateral) {
                self.profit_bidask = profit_price
            }
        }

        let open_price = self.open_bidask.get_open_price(self.is_buy);
        let close_price = self.active_bidask.get_close_price(self.is_buy);

        let diff = match self.is_buy {
            true => close_price - open_price,
            false => open_price - close_price,
        };

        let profit_price = match diff >= 0.0 {
            true => self.profit_bidask.bid,
            false => self.profit_bidask.ask,
        };

        self.pl = diff * self.lots_amount * self.contract_size * profit_price;
    }
}

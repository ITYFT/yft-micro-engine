use std::collections::{HashMap, HashSet};

use cross_calculations::core::{
    CrossCalculationsCrossPairsMatrix, CrossCalculationsError, CrossCalculationsPriceSource,
    CrossCalculationsSourceInstrument,
};

use crate::bidask::dto::MicroEngineBidask;

pub mod dto;

#[derive(Debug)]
pub struct MicroEngineBidAskCache {
    prices: HashMap<String, MicroEngineBidask>,
    base_quote_index: HashMap<String, HashMap<String, String>>,
    quote_base_index: HashMap<String, HashMap<String, String>>,
    cross_matrix: CrossCalculationsCrossPairsMatrix,
}

impl CrossCalculationsPriceSource for MicroEngineBidAskCache {
    fn get_bid_ask(
        &self,
        id: &cross_calculations::core::InstrumentId,
    ) -> Option<&impl cross_calculations::core::CrossCalculationsBidAsk> {
        self.get_by_id(id.get_source())
    }
}

impl MicroEngineBidAskCache {
    pub fn new(
        collaterals: HashSet<String>,
        instruments: Vec<MicroEngineInstrument>,
        cached_prices: Vec<MicroEngineBidask>,
    ) -> (MicroEngineBidAskCache, Vec<CrossCalculationsError>) {
        let required_crosses =
            generate_required_crosses(&instruments.iter().collect::<Vec<_>>(), collaterals);

        let (crosses, cross_errors) = CrossCalculationsCrossPairsMatrix::new(
            &required_crosses
                .iter()
                .map(|(a, b)| (a.as_str(), b.as_str()))
                .collect::<Vec<_>>(),
            &instruments.iter().collect::<Vec<_>>(),
        );

        let mut prices = HashMap::with_capacity(cached_prices.len().max(instruments.len()));
        let mut base_quote_index: HashMap<String, HashMap<String, String>> = HashMap::with_capacity(instruments.len());
        let mut quote_base_index: HashMap<String, HashMap<String, String>> = HashMap::with_capacity(instruments.len());

        for bid_ask in cached_prices {
            let id   = bid_ask.id.clone();
            let base = bid_ask.base.clone();
            let quote= bid_ask.quote.clone();

            prices.insert(id.clone(), bid_ask);

            base_quote_index.entry(base.clone()).or_default().insert(quote.clone(), id.clone());
            quote_base_index.entry(quote).or_default().insert(base, id);
        }

        (
            Self {
                prices,
                base_quote_index,
                quote_base_index,
                cross_matrix: crosses,
            },
            cross_errors,
        )
    }

    pub fn get_by_id(&self, id: &str) -> Option<&MicroEngineBidask> {
        self.prices.get(id)
    }

    pub fn get_base_quote(&self, base: &str, quote: &str) -> Option<&MicroEngineBidask> {
        let id = self.base_quote_index.get(base).and_then(|x| x.get(quote))?;

        self.prices.get(id)
    }

    pub fn get_quote_base(&self, quote: &str, base: &str) -> Option<&MicroEngineBidask> {
        let id = self.quote_base_index.get(quote).and_then(|x| x.get(base))?;

        self.prices.get(id)
    }

    pub fn get_price(&self, base: &str, quote: &str) -> Option<MicroEngineBidask> {
        if base == quote {
            return Some(MicroEngineBidask::create_blank());
        }
        let result = self
            .get_base_quote(base, quote)
            .cloned()
            .or_else(|| self.get_quote_base(base, quote).map(|x| x.reverse()));

        if result.is_none() {
            let cross = cross_calculations::core::get_cross_rate(
                base,
                quote,
                &self.cross_matrix,
                self,
                false,
            );

            if let Ok(cross) = cross {
                return Some(MicroEngineBidask::from(cross));
            }
        }

        result
    }
    
    pub fn handle_new(&mut self, bid_ask: MicroEngineBidask) {
        let id = bid_ask.id.clone();
        let base = bid_ask.base.clone();
        let quote = bid_ask.quote.clone();

        let old_price = self.prices.insert(id.clone(), bid_ask);

        if old_price.is_none() {
            let base_quote = self
                .base_quote_index
                .entry(base.clone())
                .or_default();
            base_quote.insert(quote.clone(), id.clone());

            let quote_base = self
                .quote_base_index
                .entry(quote)
                .or_default();
            quote_base.insert(base, id);
        }
    }

    pub fn get_all(&self) -> HashMap<String, MicroEngineBidask> {
        self.prices.clone()
    }

    pub fn get_price_with_source(
        &self,
        base: &str,
        quote: &str,
    ) -> Option<(MicroEngineBidask, Option<Vec<String>>)> {
        if base == quote {
            return Some((MicroEngineBidask::create_blank(), None));
        }

        if let Some(direct) = self.get_base_quote(base, quote) {
            return Some((direct.clone(), None));
        }

        if let Some(reverse) = self.get_quote_base(base, quote) {
            return Some((reverse.reverse().clone(), Some(vec![reverse.id.clone()])));
        }

        let cross =
            cross_calculations::core::get_cross_rate(base, quote, &self.cross_matrix, self, true);

        if let Ok(cross) = cross {
            let (left, right) = cross.clone().source.unwrap();
            return Some((MicroEngineBidask::from(cross), Some(vec![left.0, right.0])));
        }

        return None;
    }
}

fn generate_required_crosses(
    instruments: &[&MicroEngineInstrument],
    collaterals: HashSet<String>,
) -> Vec<(String, String)> {
    let mut crosses = HashSet::new();

    let contains_set = instruments
        .iter()
        .map(|x| format!("{}{}", x.base, x.quote))
        .collect::<HashSet<_>>();

    for instrument in instruments {
        for collateral in &collaterals {
            if instrument.base.as_str() != collateral.as_str()
                && !contains_set.contains(&format!("{}{}", instrument.base, collateral))
                && !contains_set.contains(&format!("{}{}", collateral, instrument.base))
            {
                crosses.insert((instrument.base.clone(), collateral.clone()));
            }
            if instrument.quote.as_str() != collateral.as_str()
                && !contains_set.contains(&format!("{}{}", instrument.quote, collateral))
                && !contains_set.contains(&format!("{}{}", collateral, instrument.quote))
            {
                crosses.insert((instrument.quote.clone(), collateral.clone()));
            }
        }
    }

    crosses
        .iter()
        .map(|(b, q)| (b.to_string(), q.to_string()))
        .collect::<Vec<_>>()
}

pub struct MicroEngineInstrument {
    pub id: String,
    pub base: String,
    pub quote: String,
}

impl CrossCalculationsSourceInstrument for MicroEngineInstrument {
    fn get_id(&self) -> &str {
        &self.id
    }

    fn get_base(&self) -> &str {
        &self.base
    }

    fn get_quote(&self) -> &str {
        &self.quote
    }
}

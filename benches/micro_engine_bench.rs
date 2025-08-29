use criterion::{criterion_group, criterion_main, Criterion, black_box};
use yft_micro_engine::{
    MicroEngine,
    positions::position::MicroEnginePosition,
    bidask::{dto::MicroEngineBidask, MicroEngineInstrument},
    accounts::account::MicroEngineAccount,
    settings::{MicroEngineTradingGroupSettings, TradingGroupInstrumentSettings},
};
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

fn sample_collaterals() -> HashSet<String> {
    HashSet::from(["USD".to_string()])
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
        profit_price_assets_subscriptions: HashSet::new(),
        swaps_sum: 0.0,
    }
}

fn bench_initialize(c: &mut Criterion) {
    c.bench_function("initialize", |b| {
        b.iter(|| {
            let (engine, errors) = MicroEngine::initialize(
                vec![sample_account()],
                Vec::<MicroEnginePosition>::new(),
                vec![sample_settings()],
                sample_collaterals(),
                vec![sample_instrument()],
                vec![sample_bidask()],
            );
            assert!(errors.is_empty());
            black_box(engine);
        });
    });
}

fn bench_insert_and_recalc(c: &mut Criterion) {
    let (engine, errors) = MicroEngine::initialize(
        vec![sample_account()],
        Vec::<MicroEnginePosition>::new(),
        vec![sample_settings()],
        sample_collaterals(),
        vec![sample_instrument()],
        vec![sample_bidask()],
    );
    assert!(errors.is_empty());

    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("insert_and_recalc", |b| {
        b.to_async(&rt).iter(|| async {
            let position = sample_position();
            engine.insert_or_update_position(position).await.unwrap();
            engine.handle_new_price(vec![sample_bidask()]).await;
            engine.recalculate_accordint_to_updates().await;
        });
    });
}

fn bench_handle_bidask(c: &mut Criterion) {
    let (engine, errors) = MicroEngine::initialize(
        vec![sample_account()],
        Vec::<MicroEnginePosition>::new(),
        vec![sample_settings()],
        sample_collaterals(),
        vec![sample_instrument()],
        vec![sample_bidask()],
    );
    assert!(errors.is_empty());

    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("handle_bidask", |b| {
        b.to_async(&rt).iter(|| async {
            engine.handle_new_price(vec![sample_bidask()]).await;
        });
    });
}

criterion_group!(
    benches,
    bench_initialize,
    bench_insert_and_recalc,
    bench_handle_bidask
);
criterion_main!(benches);
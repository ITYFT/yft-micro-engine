use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use criterion::{
    BatchSize, BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main,
};
use tokio::runtime::Builder;

use yft_micro_engine::{
    MicroEngine,
    accounts::account::MicroEngineAccount,
    bidask::{MicroEngineInstrument, dto::MicroEngineBidask},
    positions::position::MicroEnginePosition,
    settings::{MicroEngineTradingGroupSettings, TradingGroupInstrumentSettings},
};

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
        profit_price_assets_subscriptions: Vec::new(),
        swaps_sum: 0.0,
    }
}

fn build_engine() -> Arc<MicroEngine> {
    let rt = Builder::new_current_thread().enable_all().build().unwrap();

    let (engine, _errors) = rt.block_on(MicroEngine::initialize(
        vec![sample_account()],
        Vec::<MicroEnginePosition>::new(),
        vec![sample_settings()],
        sample_collaterals(),
        vec![sample_instrument()],
        vec![sample_bidask()],
    ));
    Arc::new(engine)
}

fn gen_prices_unique(n: usize) -> Vec<MicroEngineBidask> {
    (0..n)
        .map(|i| MicroEngineBidask {
            id: format!("EURUSD{i}"),
            bid: 1.0 + (i as f64) * 1e-6,
            ask: 1.1 + (i as f64) * 1e-6,
            base: "EUR".into(),
            quote: "USD".into(),
        })
        .collect()
}

fn gen_positions(n: usize) -> Vec<MicroEnginePosition> {
    let px = sample_bidask();
    (0..n)
        .map(|i| MicroEnginePosition {
            id: format!("POS{i}"),
            trader_id: "TR1".into(),
            account_id: "ACC1".into(),
            base: "EUR".into(),
            quote: "USD".into(),
            collateral: "USD".into(),
            asset_pair: "EURUSD".into(),
            lots_amount: 1.0,
            contract_size: 1.0,
            is_buy: i % 2 == 0,
            pl: 0.0,
            commission: 0.0,
            open_bidask: px.clone(),
            active_bidask: px.clone(),
            margin_bidask: px.clone(),
            profit_bidask: MicroEngineBidask::create_blank(),
            profit_price_assets_subscriptions: Vec::new(),
            swaps_sum: 0.0,
        })
        .collect()
}

fn bench_initialize(c: &mut Criterion) {
    let rt = Builder::new_current_thread().enable_all().build().unwrap();
    c.bench_function("initialize", |b| {
        b.iter(|| {
            let (engine, errors) = rt.block_on(MicroEngine::initialize(
                vec![sample_account()],
                Vec::<MicroEnginePosition>::new(),
                vec![sample_settings()],
                sample_collaterals(),
                vec![sample_instrument()],
                vec![sample_bidask()],
            ));
            assert!(errors.is_empty());
            black_box(engine);
        });
    });
}

// fn bench_recalc_after_single_price(c: &mut Criterion) {
//     let rt = Builder::new_current_thread().enable_all().build().unwrap();
//     let engine = build_engine();
//     rt.block_on(async {
//         engine
//             .insert_or_update_position(sample_position())
//             .await
//             .unwrap();
//     });

//     c.bench_function("recalc_after_single_price", |b| {
//         b.to_async(&rt).iter(|| async {
//             engine.handle_new_price(vec![sample_bidask()]).await;
//             let _ = engine.recalculate_accordint_to_updates().await;
//         });
//     });
// }

// fn bench_handle_bidask_hot(c: &mut Criterion) {
//     let rt = Builder::new_current_thread().enable_all().build().unwrap();
//     let engine = build_engine();

//     c.bench_function("handle_bidask/hot_same_id", |b| {
//         b.to_async(&rt).iter(|| async {
//             engine
//                 .handle_new_price(black_box(vec![sample_bidask()]))
//                 .await;
//         });
//     });
// }

// fn bench_handle_new_price_large_batches_fresh(c: &mut Criterion) {
//     let rt = Builder::new_current_thread().enable_all().build().unwrap();
//     let mut group = c.benchmark_group("handle_new_price/large_batches_fresh_engine");
//     for &n in &[1_000usize, 10_000, 50_000] {
//         group.throughput(Throughput::Elements(n as u64));
//         group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
//             b.to_async(&rt).iter_batched(
//                 || (build_engine(), black_box(gen_prices_unique(n))),
//                 |(engine, prices)| async move {
//                     engine.handle_new_price(prices).await;
//                 },
//                 BatchSize::LargeInput,
//             );
//         });
//     }
//     group.finish();
// }

// fn bench_handle_new_price_heavy_state(c: &mut Criterion) {
//     let rt = Builder::new_current_thread().enable_all().build().unwrap();

//     let engine = {
//         let (e, _errors) = rt.block_on(MicroEngine::initialize(
//             vec![sample_account()],
//             gen_positions(10_000),
//             vec![sample_settings()],
//             sample_collaterals(),
//             vec![sample_instrument()],
//             vec![sample_bidask()],
//         ));
//         Arc::new(e)
//     };

//     let mut group = c.benchmark_group("handle_new_price/heavy_state");
//     for &n in &[1_000usize, 10_000, 50_000] {
//         group.throughput(Throughput::Elements(n as u64));
//         group.bench_with_input(BenchmarkId::from_parameter(n), &n, |b, &n| {
//             b.to_async(&rt).iter_batched(
//                 || black_box(gen_prices_unique(n)),
//                 |prices| {
//                     let engine = engine.clone();
//                     async move {
//                         engine.handle_new_price(prices).await;
//                         let _ = engine.recalculate_accordint_to_updates().await;
//                     }
//                 },
//                 BatchSize::LargeInput,
//             );
//         });
//     }
//     group.finish();
// }

criterion_group!(
    benches,
    bench_initialize // bench_recalc_after_single_price,
                     // bench_handle_bidask_hot,
                     // bench_handle_new_price_large_batches_fresh,
                     // bench_handle_new_price_heavy_state
);
criterion_main!(benches);

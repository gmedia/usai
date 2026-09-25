// Measures what `Engine::release_idle_pool_memory` actually gives back.
//
// Warm N pooling slots by instantiating N modules that dirty a few MiB each,
// drop them all, then sample RSS before and after the release.
use wasmtime::*;

fn rss_kib() -> u64 {
    let s = std::fs::read_to_string("/proc/self/statm").unwrap();
    let pages: u64 = s.split_whitespace().nth(1).unwrap().parse().unwrap();
    pages * (rustix_page_size() as u64) / 1024
}
fn rustix_page_size() -> usize {
    unsafe { libc_sysconf() }
}
#[cfg(unix)]
unsafe fn libc_sysconf() -> usize {
    unsafe extern "C" { fn sysconf(name: i32) -> i64; }
    unsafe { sysconf(30) as usize } // _SC_PAGESIZE on Linux
}

const WAT: &str = r#"
(module
  (memory (export "memory") 64)
  (func (export "dirty") (param i32)
    (local $i i32)
    (loop $l
      (i32.store (local.get $i) (local.get $i))
      (local.set $i (i32.add (local.get $i) (i32.const 4096)))
      (br_if $l (i32.lt_u (local.get $i) (local.get 0)))))
)
"#;

fn main() -> Result<()> {
    let slots: u32 = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(64);
    let dirty_bytes: i32 = 4 * 1024 * 1024;

    let mut pool = PoolingAllocationConfig::new();
    pool.total_memories(slots);
    pool.max_memory_size(64 * 1024 * 1024);
    pool.total_tables(slots);
    pool.table_elements(1);
    pool.linear_memory_keep_resident(8 * 1024 * 1024);
    let mut config = Config::new();
    config.allocation_strategy(InstanceAllocationStrategy::Pooling(pool));
    let engine = Engine::new(&config)?;
    let module = Module::new(&engine, WAT)?;
    let metrics = engine.pooling_allocator_metrics().unwrap();

    // Warm every slot, dirtying part of each.
    {
        let mut stores = Vec::new();
        for _ in 0..slots {
            let mut store = Store::new(&engine, ());
            let instance = Instance::new(&mut store, &module, &[])?;
            instance
                .get_typed_func::<i32, ()>(&mut store, "dirty")?
                .call(&mut store, dirty_bytes)?;
            stores.push(store);
        }
        // instances dropped here: every slot goes back warm
    }

    // `RELEASE=0` is the control: idle for the same time and do not call.
    let call = std::env::var("RELEASE").as_deref() != Ok("0");
    let before = rss_kib();
    let resident = metrics.unused_memory_bytes_resident();
    std::thread::sleep(std::time::Duration::from_secs(5));
    let released = if call { engine.release_idle_pool_memory() } else { 0 };
    let after = rss_kib();
    println!("release called                      : {call}");

    println!("slots warmed              : {slots}");
    println!("unused_memory_bytes_resident before : {:.1} MiB", resident as f64 / 1048576.0);
    println!("released by the call                : {:.1} MiB", released as f64 / 1048576.0);
    println!("unused_memory_bytes_resident after  : {:.1} MiB",
             metrics.unused_memory_bytes_resident() as f64 / 1048576.0);
    println!("process RSS before                  : {:.1} MiB", before as f64 / 1024.0);
    println!("process RSS after                   : {:.1} MiB", after as f64 / 1024.0);
    println!("RSS delta                           : -{:.1} MiB",
             (before as f64 - after as f64) / 1024.0);

    // The other half of the trade: what the first instantiation after the
    // quiet period costs, and what the one after that costs once the slot is
    // warm again.
    let mut first = std::time::Duration::ZERO;
    let mut second = std::time::Duration::ZERO;
    for (n, slot) in [(0usize, &mut first), (1usize, &mut second)] {
        let t = std::time::Instant::now();
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[])?;
        instance
            .get_typed_func::<i32, ()>(&mut store, "dirty")?
            .call(&mut store, dirty_bytes)?;
        *slot = t.elapsed();
        let _ = n;
    }
    println!("first instantiate + 4 MiB touch     : {:.2} ms", first.as_secs_f64() * 1e3);
    println!("second (slot warm again)            : {:.2} ms", second.as_secs_f64() * 1e3);
    Ok(())
}

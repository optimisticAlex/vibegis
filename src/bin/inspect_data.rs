//! Native validation/benchmark of the same Store used by the WASM application.
#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{
        collections::HashSet,
        fs::File,
        io::{BufReader, Read},
        time::Instant,
    };
    use vibegis::model::{HEADER, House, Store};
    let path = std::env::args()
        .nth(1)
        .ok_or("Usage: cargo run --release --bin inspect_data -- FILE.csv[.gz] [LIMIT]")?;
    let limit = std::env::args()
        .nth(2)
        .map(|x| x.parse::<usize>())
        .transpose()?
        .unwrap_or(usize::MAX);
    let file = File::open(&path)?;
    let input: Box<dyn Read> = if path.ends_with(".gz") {
        Box::new(flate2::read::GzDecoder::new(file))
    } else {
        Box::new(file)
    };
    let mut reader = csv::Reader::from_reader(BufReader::new(input));
    if reader.headers()?.iter().ne(HEADER) {
        return Err("Unexpected header".into());
    }
    let start = Instant::now();
    let mut store = Store::default();
    let mut seen = HashSet::new();
    let mut record = csv::StringRecord::new();
    while store.houses.len() < limit && reader.read_record(&mut record)? {
        let house = House::from_record(&record)?;
        if !seen.insert(house.id) {
            return Err("Duplicate ID".into());
        }
        store.push(house)?;
    }
    let load_seconds = start.elapsed().as_secs_f64();
    drop(seen);
    let start = Instant::now();
    let indices = store.select(&[[12.08, 49.0], [12.12, 49.0], [12.12, 49.03], [12.08, 49.03]])?;
    let selection_ms = start.elapsed().as_secs_f64() * 1000.;
    println!(
        "{}",
        serde_json::json!({"file":path,"rows":store.houses.len(),"status_totals":store.totals,"load_seconds":load_seconds,"regensburg_selection":indices.len(),"selection_ms":selection_ms,"house_bytes":std::mem::size_of::<House>()})
    );
    Ok(())
}
#[cfg(target_arch = "wasm32")]
fn main() {}

// T7 example: load CSV, filter, group_by + agg, pretty-print.
// The matching dataframe_csv.buff mirrors this pipeline using the
// Buff language surface (`DataFrame.from_csv` / `df.filter` / etc).
use buff_dataframe::{AggOp, DataFrame};

fn main() {
    let df = DataFrame::from_csv("examples/dataframe/sample.csv").unwrap_or_default();
    println!("Loaded rows: {}", df.len());

    let london = df
        .filter(|row| row.get_string("city") == Some("London"))
        .unwrap_or_default();
    println!("London rows:");
    println!("{}", london.to_table_string());

    let by_city = df.group_by("city").unwrap();
    let mean_age = by_city.agg("age", AggOp::Mean);
    println!("Mean age by city:");
    println!("{}", mean_age.to_table_string());
}

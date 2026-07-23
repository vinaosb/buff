// T7 example: load JSON-lines, select, sort, head, pretty-print.
// The matching dataframe_json.buff mirrors this pipeline using the
// Buff language surface (`DataFrame.from_json` / `df.select` / etc).
use buff_dataframe::DataFrame;

fn main() {
    let df = DataFrame::from_json("examples/dataframe/rows.jsonl").unwrap_or_default();
    println!("Loaded rows: {}", df.len());

    let projected = df.select(&["name", "score"]).unwrap_or_default();
    println!("Projected columns:");
    println!("{}", projected.to_table_string());

    let ranked = projected.sort("score").unwrap_or_default().head(5);
    println!("Top 5 by score:");
    println!("{}", ranked.to_table_string());
}

use serde_json::{json, Value};
use std::io::{self, BufRead, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    for line in stdin.lock().lines() {
        let input: Value = serde_json::from_str(&line?)?;
        let left = u32::try_from(input["left"].as_u64().ok_or("left must be u32")?)?;
        let right = u32::try_from(input["right"].as_u64().ok_or("right must be u32")?)?;
        let budget = usize::try_from(
            input["budget"]
                .as_u64()
                .ok_or("budget must be portable usize")?,
        )?;
        let operation = input["operation"]
            .as_str()
            .ok_or("operation must be a string")?;
        let result = match lattice_web::evaluate(left, right, budget, operation) {
            Ok(value) => json!({"Ok": value.to_string()}),
            Err(message) => json!({"Err": message}),
        };
        writeln!(output, "{result}")?;
    }
    Ok(())
}

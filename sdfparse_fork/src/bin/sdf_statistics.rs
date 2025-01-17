use sdfparse::SDF;
use std::env;
use std::fs;

fn main() {
    clilog::init_stderr_color_debug();
    let args: Vec<String> = env::args().collect();
    assert!(args.len() == 2,
            "Usage: {} <sdf_path>", args[0]);

    let sdf = fs::read_to_string(&args[1])
        .expect("Error reading sdf source file");

    let sdf = match SDF::parse_str(&sdf) {
        Ok(sdf) => sdf,
        Err(e) => panic!("{}", e)
    };

    clilog::info!("SDF file {}", args[1]);
    clilog::info!("VERSION {:?}", sdf.header.sdf_version);
    clilog::info!("DESIGN {:?}, CREATED BY {:?} {:?} {:?}",
                  sdf.header.design_name, sdf.header.vendor, sdf.header.program, sdf.header.program_version);
    clilog::info!("# Cells = {}", sdf.cells.len());
    clilog::info!("# Delays  = {}", sdf.cells.iter().map(|c| c.delays.len()).sum::<usize>());
}


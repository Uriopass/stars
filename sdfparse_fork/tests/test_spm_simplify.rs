use sdfparse::*;

const SDF_SPM: &str = include_str!("spm_simplify.sdf");

#[test]
fn test_spm_simplify() {
    let sdf = match SDF::parse_str(SDF_SPM) {
        Ok(sdf) => sdf,
        Err(e) => panic!("Parsing error: {e}")
    };
    assert_eq!(format!("{:?}", sdf.header), "SDFHeader { sdf_version: \"3.0\", design_name: Some(\"spm\"), date: Some(\"Wed Oct 13 19:52:19 2021\"), vendor: Some(\"Parallax\"), program: Some(\"STA\"), program_version: Some(\"2.3.0\"), hier_divider: '/', voltage: Some(Multi(Some(1.95), None, Some(1.95))), process: Some(\"1.000::1.000\"), temperature: Some(Multi(Some(-40.0), None, Some(-40.0))), timescale: 1e-9 }");

    assert_eq!(sdf.cells.len(), 4);
    assert_eq!(sdf.cells[0].celltype, "spm");
    assert!(sdf.cells[0].instance.is_none());
    assert_eq!(sdf.cells[0].delays.len(), 4);
    assert_eq!(format!("{:?}", sdf.cells[0].delays[3]), "Interconnect(SDFDelayInterconnect { a: SDFPath { path: [\"input1\", \"X\"], bus: None }, b: SDFPath { path: [\"_182_\", \"A\"], bus: SingleBit(1) }, delay: [Multi(Some(0.00019543248), None, Some(0.00019546332)), Multi(Some(0.00018196118), None, Some(0.00018203554))] })");

    assert_eq!(format!("{:?}", sdf.cells[3].delays[2]), "IOPath(Cond([(SDFPort { port_name: \"SD\", bus: None }, false), (SDFPort { port_name: \"SLP\", bus: None }, false), (SDFPort { port_name: \"BIST\", bus: None }, true), (SDFPort { port_name: \"CEBM\", bus: None }, false), (SDFPort { port_name: \"WEBM\", bus: None }, true)]), SDFDelayIOPath { a: SDFPortSpec { edge_type: Posedge, port: SDFPort { port_name: \"CLK\", bus: None } }, b: SDFPort { port_name: \"Q\", bus: SingleBit(9) }, retain: Some([Multi(Some(0.789), None, Some(0.789)), Multi(Some(0.789), None, Some(0.789))]), delay: [Multi(Some(0.984), None, Some(0.984)), Multi(Some(0.984), None, Some(0.984))] })");
}

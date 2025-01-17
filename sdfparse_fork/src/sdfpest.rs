//! Pest token to data structure.

use super::*;
use pest::Parser;
use pest_derive::Parser;
use std::str::FromStr;
use parsing_utils::PairsHelper;

#[derive(Parser)]
#[grammar = "sdf.pest"]
struct SDFParser;

type Pair<'i> = pest::iterators::Pair<'i, Rule>;

#[inline]
fn unescape(s: &str) -> CompactString {
    if s.chars().all(|c| c != '\\') {
        return s.into();
    }
    let mut cs = CompactString::with_capacity(s.len());
    let mut s = s.chars();
    while let Some(c) = s.next() {
        if c == '\\' { cs.push(s.next().unwrap()); }
        else { cs.push(c); }
    }
    cs
}

#[inline]
fn parse_str(p: Pair) -> CompactString {
    assert_eq!(p.as_rule(), Rule::str);
    let substr = p.as_str();
    let substr = &substr[1..substr.len() - 1];
    unescape(substr)
}

#[inline]
fn parse_ident(p: Pair) -> CompactString {
    assert_eq!(p.as_rule(), Rule::ident);
    let substr = p.as_str();
    unescape(substr)
}

#[inline]
fn parse_int(p: Pair) -> isize {
    assert_eq!(p.as_rule(), Rule::int);
    isize::from_str(p.as_str()).unwrap()
}

#[inline]
fn parse_real(p: Pair) -> f32 {
    assert_eq!(p.as_rule(), Rule::real);
    f32::from_str(p.as_str()).unwrap()
}

#[inline]
fn parse_rvalue(p: Pair) -> SDFValue {
    assert_eq!(p.as_rule(), Rule::rvalue);
    let p = unwrap_one(p);
    match p.as_rule() {
        Rule::real_optional => {
            match p.into_inner().next() {
                Some(p) => SDFValue::Single(parse_real(p)),
                None => SDFValue::None
            }
        },
        Rule::rvalue_multi => {
            let mut p = PairsHelper(p.into_inner());
            SDFValue::Multi(
                p.next().into_inner().next().map(parse_real),
                p.next().into_inner().next().map(parse_real),
                p.next().into_inner().next().map(parse_real)
            )
        },
        _ => unreachable!()
    }
}

#[inline]
fn parse_rvalue_list(p: Pair) -> Vec<SDFValue> {
    p.into_inner().map(parse_rvalue).collect()
}

#[inline]
fn parse_char(p: Pair) -> char {
    assert!(p.as_rule() == Rule::hchar);
    let s = p.as_str();
    assert_eq!(s.len(), 1);
    s.chars().next().unwrap()
}

#[inline]
fn unwrap_one(p: Pair) -> Pair {
    let mut p = PairsHelper(p.into_inner());
    p.next()
}

#[inline]
fn parse_bus(p: Pair) -> SDFBus {
    assert_eq!(p.as_rule(), Rule::bus);
    let mut p = PairsHelper(p.into_inner());
    let l = parse_int(p.next());
    match p.next_rule_opt(Rule::int) {
        Some(p) => SDFBus::BitRange(l, parse_int(p)),
        None => SDFBus::SingleBit(l)
    }
}

#[inline]
fn parse_path(p: Pair) -> SDFPath {
    assert_eq!(p.as_rule(), Rule::path);
    let mut p = PairsHelper(p.into_inner());
    SDFPath {
        path: p.iter_while(Rule::ident).map(parse_ident).collect(),
        bus: p.next_rule_opt(Rule::bus).map(parse_bus)
            .unwrap_or(SDFBus::None)
    }
}

#[inline]
fn parse_port(p: Pair) -> SDFPort {
    assert_eq!(p.as_rule(), Rule::port);
    let mut p = PairsHelper(p.into_inner());
    SDFPort {
        port_name: parse_ident(p.next()),
        bus: p.next_rule_opt(Rule::bus).map(parse_bus)
            .unwrap_or(SDFBus::None)
    }
}

#[inline]
fn parse_port_spec(p: Pair) -> SDFPortSpec {
    assert_eq!(p.as_rule(), Rule::port_spec);
    let mut p = PairsHelper(p.into_inner());
    use SDFPortEdge::*;
    SDFPortSpec {
        edge_type: p.next_rule_opt(Rule::port_edge_type)
            .map(|p| match p.as_str() {
                "posedge" => Posedge, "negedge" => Negedge,
                "01" => T01, "10" => T10, "0z" => T0Z, "z1" => TZ1,
                "1z" => T1Z, "z0" => TZ0,
                _ => unreachable!()
            })
            .unwrap_or(SDFPortEdge::None),
        port: parse_port(p.next())
    }
}

#[inline]
fn parse_header(p: Pair) -> SDFHeader {
    assert_eq!(p.as_rule(), Rule::header);
    let mut p = PairsHelper(p.into_inner());
    macro_rules! parse_fields {
        ($($($field:ident)|+ => $parse:expr),+) => {
            $($(let $field = p.next_rule_opt(Rule::$field)
              .map(|p| $parse(unwrap_one(p)));)+)+
        }
    }
    parse_fields! {
        sdf_version | design_name | date |
        vendor | program | program_version
            => parse_str,
        hier_divider => parse_char,
        voltage => parse_rvalue,
        process => parse_str,
        temperature => parse_rvalue
    }
    let timescale = p.next_rule_opt(Rule::timescale).map(|p| {
        let mut p = PairsHelper(p.into_inner());
        parse_real(p.next()) * match p.next().as_str() {
            "us" => 1e-6, "ns" => 1e-9, "ps" => 1e-12,
            _ => unreachable!()
        }
    }).unwrap_or(1e-9); // default 1ns
    SDFHeader {
        sdf_version: sdf_version.unwrap(),
        design_name, date, vendor,
        program, program_version,
        hier_divider: hier_divider.unwrap(),
        voltage, process, temperature,
        timescale
    }
}

fn parse_delay_interconnect(p: Pair) -> SDFDelayInterconnect {
    assert_eq!(p.as_rule(), Rule::delay_interconnect);
    let mut p = PairsHelper(p.into_inner());
    SDFDelayInterconnect {
        a: parse_path(p.next()),
        b: parse_path(p.next()),
        delay: parse_rvalue_list(p.next())
    }
}

fn parse_delay_iopath(p: Pair) -> SDFDelayIOPath {
    assert_eq!(p.as_rule(), Rule::delay_iopath);
    let mut p = PairsHelper(p.into_inner());
    SDFDelayIOPath {
        a: parse_port_spec(p.next()),
        b: parse_port(p.next()),
        retain: p.next_rule_opt(Rule::delay_iopath_retain).map(
            |p| parse_rvalue_list(unwrap_one(p))
        ),
        delay: parse_rvalue_list(p.next())
    }
}

#[inline]
fn parse_iopath_cond_expr(p: Pair) -> Vec<(SDFPort, bool)> {
    assert_eq!(p.as_rule(), Rule::cond_expr);
    p.into_inner().map(|p| {
        let val = match p.as_rule() {
            Rule::cond_expr_inst_neg => false,
            Rule::cond_expr_inst_pos => true,
            _ => unreachable!()
        };
        (parse_port(unwrap_one(p)), val)
    }).collect()
}

#[inline]
fn parse_delay(p: Pair) -> SDFDelay {
    let p = unwrap_one(p);
    match p.as_rule() {
        Rule::delay_interconnect => SDFDelay::Interconnect(
            parse_delay_interconnect(p)
        ),
        Rule::delay_iopath => SDFDelay::IOPath(
            SDFIOPathCond::None,
            parse_delay_iopath(p)
        ),
        Rule::delay_cond_iopath => {
            let mut p = PairsHelper(p.into_inner());
            SDFDelay::IOPath(
                SDFIOPathCond::Cond(parse_iopath_cond_expr(p.next())),
                parse_delay_iopath(p.next())
            )
        },
        Rule::delay_condelse_iopath => SDFDelay::IOPath(
            SDFIOPathCond::CondElse,
            parse_delay_iopath(unwrap_one(p))
        ),
        _ => unreachable!()
    }
}

fn parse_cell(p: Pair) -> SDFCell {
    let mut p = PairsHelper(p.into_inner());
    let celltype = parse_str(p.next());
    let instance = p.next_rule_opt(Rule::path).map(parse_path);
    let mut delays = Vec::new();
    for timing_spec in p.iter_while(Rule::timing_spec).map(unwrap_one) {
        match timing_spec.as_rule() {
            Rule::delay => {
                delays.extend(timing_spec.into_inner()
                              .map(parse_delay));
            },
            Rule::timingcheck => {
                // TODO: timingcheck not parsed here.
                drop(timing_spec);
            },
            _ => unreachable!()
        }
    }
    SDFCell {
        celltype,
        instance,
        delays
    }
}

pub(crate) fn parse_sdf(s: &str) -> Result<SDF, String> {
    let p = match SDFParser::parse(Rule::main, s) {
        Ok(mut r) => r.next().unwrap(),
        Err(e) => return Err(format!("{}", e)),
    };
    let mut p = PairsHelper(p.into_inner());
    Ok(SDF {
        header: parse_header(p.next()),
        cells: p.iter_while(Rule::cell).map(parse_cell).collect()
    })
}

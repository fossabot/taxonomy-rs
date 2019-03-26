use std::collections::HashMap;
use std::fmt::{Debug, Display};
use std::fs::File;
use std::hash::Hash;
use std::io::{Read, Write};
use std::iter::Sum;
use std::path::Path;
use std::str::FromStr;

use failure::{bail, format_err};
use serde_json::{from_reader, json, to_writer, Value};

use crate::base::GeneralTaxonomy;
use crate::rank::TaxRank;
use crate::taxonomy::Taxonomy;
use crate::Result;

// TODO: move to the CLI or Python API?
pub fn load_json_file<P>(build_json_path: P, key: Option<&str>) -> Result<GeneralTaxonomy>
where
    P: AsRef<Path>,
{
    let build_file =
        File::open(build_json_path).map_err(|_| format_err!("build.json not found"))?;
    load_json(build_file, key)
}

fn extract_json_node<'k, 'n>(node: &'n Value, key: &'k str) -> Result<&'n Value> {
    let mut mnode = node;
    for subkey in key.split('.') {
        if let Some(n) = mnode.get(subkey) {
            mnode = n;
        } else {
            bail!("JSON key {} does not exist", subkey);
        }
    }
    Ok(mnode)
}

/// Load a Taxonomy out of the `reader` automatically trying to determine
/// what subtype of the JSON types we understand it is.
pub fn load_json<R>(reader: R, key: Option<&str>) -> Result<GeneralTaxonomy>
where
    R: Read,
{
    let tax_json: Value = from_reader(reader)?;
    let tax_subjson: &Value = if let Some(real_key) = key {
        extract_json_node(&tax_json, real_key)?
    } else {
        &tax_json
    };

    // determine the JSON type
    if tax_subjson.get("nodes") != None {
        load_node_link_json(tax_subjson)
    } else {
        load_tree_json(tax_subjson)
    }
}

pub fn load_node_link_json(tax_json: &Value) -> Result<GeneralTaxonomy> {
    let tax_links = tax_json["links"]
        .as_array()
        .ok_or_else(|| format_err!("'links' not in JSON"))?;
    let tax_nodes = tax_json["nodes"]
        .as_array()
        .ok_or_else(|| format_err!("'nodes' not in JSON"))?;
    let mut parent_ids = vec![0; tax_nodes.len()];
    for l in tax_links {
        // TODO: some error messages for these unwraps
        let source = l["source"].as_u64().unwrap() as usize;
        let target = l["target"].as_u64().unwrap() as usize;
        parent_ids[source] = target;
    }

    let mut tax_ids = Vec::new();
    let mut names = Vec::new();
    let mut ranks = Vec::new();
    for n in tax_nodes.iter() {
        let ncbi_id = n["id"].as_u64().map_or_else(
            || {
                n["id"]
                    .as_str()
                    .ok_or_else(|| format_err!("IDs must be strings or numbers"))
                    .map(|id| id.to_string())
            },
            |id| Ok(id.to_string()),
        )?;
        tax_ids.push(ncbi_id);
        let name = n["name"]
            .as_str()
            .ok_or_else(|| format_err!("Names must be strings"))?
            .to_string();
        names.push(name);
        let rank = if n["rank"].is_null() {
            None
        } else {
            TaxRank::from_str(
                n["rank"]
                    .as_str()
                    .ok_or_else(|| format_err!("Ranks must be strings"))?,
            )
            .ok()
        };
        ranks.push(rank);
    }

    Ok(GeneralTaxonomy::new(
        tax_ids,
        parent_ids,
        Some(names),
        Some(ranks),
        None,
    ))
}

pub fn load_tree_json(tax_json: &Value) -> Result<GeneralTaxonomy> {
    fn add_node(
        parent_loc: usize,
        node: &Value,
        tax_ids: &mut Vec<String>,
        parent_ids: &mut Vec<usize>,
        names: &mut Vec<String>,
        ranks: &mut Vec<Option<TaxRank>>,
    ) -> Result<()> {
        let tax_id = node["id"].to_string();
        tax_ids.push(tax_id.clone());
        parent_ids.push(parent_loc);
        names.push(
            node["name"]
                .as_str()
                .ok_or_else(|| format_err!("Name for {} not a string", tax_id))?
                .to_string(),
        );
        let rank = node["rank"]
            .as_str()
            .ok_or_else(|| format_err!("Rank for {} is not a string", tax_id))?;
        ranks.push(TaxRank::from_str(rank).ok());

        let loc = tax_ids.len() - 1;
        for child in node["children"]
            .as_array()
            .ok_or_else(|| format_err!("Children for {} is not an array", tax_id))?
        {
            add_node(loc, child, tax_ids, parent_ids, names, ranks)?;
        }
        Ok(())
    }

    // it doesn't matter what the first node's parent is so we loop to self
    let mut tax_ids = Vec::new();
    let mut parent_ids = Vec::new();
    let mut names = Vec::new();
    let mut ranks = Vec::new();

    // it doesn't matter what the first node's parent is so we loop to self
    add_node(
        0,
        tax_json,
        &mut tax_ids,
        &mut parent_ids,
        &mut names,
        &mut ranks,
    )?;

    Ok(GeneralTaxonomy::new(
        tax_ids,
        parent_ids,
        Some(names),
        Some(ranks),
        None,
    ))
}

pub fn save_json<'t, T: 't, D: 't, X, W>(
    tax: &'t X,
    writer: W,
    root_node: Option<T>,
    as_node_link: bool,
) -> Result<()>
where
    T: Clone + Debug + Display + Eq + Hash + PartialEq,
    D: Debug + PartialOrd + Sum,
    X: Taxonomy<'t, T, D>,
    W: Write,
{
    let json_data = if as_node_link {
        save_node_link_json(tax, root_node)?
    } else {
        save_tree_json(tax, root_node)?
    };
    to_writer(writer, &json_data)?;
    Ok(())
}

pub fn save_node_link_json<'t, T: 't, D: 't, X>(tax: &'t X, root_node: Option<T>) -> Result<Value>
where
    T: Clone + Debug + Display + Eq + Hash + PartialEq,
    D: Debug + PartialOrd + Sum,
    X: Taxonomy<'t, T, D>,
{
    let root_id = if let Some(tid) = root_node {
        tid
    } else {
        tax.root()
    };
    let mut id_to_idx: HashMap<T, usize> = HashMap::new();

    let mut nodes: Vec<Value> = Vec::new();
    let mut links: Vec<Value> = Vec::new();
    for (ix, (tid, _pre)) in tax.traverse(root_id)?.filter(|x| x.1).enumerate() {
        let tax_id = format!("{}", tid);
        let name = tax.name(tid.clone())?;
        let rank = tax.rank(tid.clone())?;
        nodes.push(json!({
            "name": name,
            "rank": rank.map_or("", |x| x.to_ncbi_rank()),
            "id": tax_id,
        }));
        id_to_idx.insert(tid.clone(), ix);
        if let Some(parent_id) = tax.parent(tid)? {
            links.push(json!({
                "source": ix,
                "target": id_to_idx[&parent_id.0],
            }));
        }
    }
    let tax_json = json!({
        "nodes": nodes,
        "links": links,
        "directed": true,
        "multigraph": false,
        "graph": [],
    });

    Ok(tax_json)
}

pub fn save_tree_json<'t, T: 't, D: 't, X>(tax: &'t X, root_node: Option<T>) -> Result<Value>
where
    T: Clone + Debug + Display + PartialEq,
    D: Debug + PartialOrd + Sum,
    X: Taxonomy<'t, T, D>,
{
    let tax_id = if let Some(tid) = root_node {
        tid
    } else {
        tax.root()
    };
    let mut children: Vec<Value> = Vec::new();
    for child in tax.children(tax_id.clone())? {
        children.push(save_tree_json(tax, Some(child))?);
    }

    let name = tax.name(tax_id.clone())?;
    let rank = tax.rank(tax_id.clone())?;
    let tax_json = json!({
        "id": format!("{}", tax_id.clone()),
        "name": name,
        "rank": rank.map_or("", |x| x.to_ncbi_rank()),
        "children": children,
    });

    Ok(tax_json)
}

//! Reads and writes [SpatioTemporal Asset Catalogs (STACs)](https://stacspec.org/) in Rust.
//!
//! The SpatioTemporal Asset Catalog (STAC) specification provides a common language to describe a range of geospatial information, so it can more easily be indexed and discovered.
//! A 'spatiotemporal asset' is any file that represents information about the earth captured in a certain space and time.
//!
//! The goal is for all providers of spatiotemporal assets (Imagery, SAR, Point Clouds, Data Cubes, Full Motion Video, etc) to expose their data as SpatioTemporal Asset Catalogs (STAC), so that new code doesn't need to be written whenever a new data set or API is released.
//!
//! This is a Rust implementation of the specification, with associated utilities.
//! Similar projects in other languages include:
//!
//! - Python: [PySTAC](https://pystac.readthedocs.io/en/1.0/)
//! - Go: [go-stac](https://github.com/planetlabs/go-stac)
//! - .NET: [DotNetStac](https://github.com/Terradue/DotNetStac)
//! - PHP: [resto](https://github.com/jjrom/resto)
//!
//! # Basic data strutures
//!
//! STAC is built on three data structures:
//!
//! - [Item](https://github.com/radiantearth/stac-spec/blob/master/item-spec/item-spec.md) is a [GeoJSON](http://geojson.org/) [Feature](https://tools.ietf.org/html/rfc7946#section-3.2) augmented with [foreign members](https://tools.ietf.org/html/rfc7946#section-6) relevant to a STAC object.
//! - [Catalog](https://github.com/radiantearth/stac-spec/blob/master/catalog-spec/catalog-spec.md) represents a logical group of other Catalog, Collection, and Item objects.
//! - [Collection](https://github.com/radiantearth/stac-spec/blob/master/collection-spec/collection-spec.md) shares all fields with the Catalog (with different allowed values for `type` and `stac_extensions`) and adds fields to describe the whole dataset and the included set of Items.
//!
//! All three are provided as [serde](https://serde.rs/) (de)serializable structures with public attributes.
//! Because `id` is always required, these structures do not implement [Default], but instead provides a `new` method that takes an `id` and fills the rest of the object's attributes with sensible defaults.
//!
//! ```
//! use stac::{Item, Catalog, Collection};
//! let item = Item::new("id");
//! let catalog = Catalog::new("id");
//! let collection = Catalog::new("id");
//! ```
//!
//! # Reading and writing
//!
//! Because STAC is used to access and write data on local and remote filesystems, this crate provides flexibility for downstream users to customize their input and output operations.
//! The [Read] trait provides an interface that turns [Hrefs](Href) into STAC objects.
//! The provided [Reader] uses the standard library for filesystem access and [reqwest](https://docs.rs/reqwest/latest/reqwest/) for network access, if enabled by the `reqwest` feature, which is enabled by default:
//!
//! ```
//! use stac::{Reader, Read};
//! let catalog = Reader::default().read("data/catalog.json").unwrap();
//! ```
//!
//! Because the type of a STAC object cannot be known before reading, the [Read] trait returns an [Object], which is a wrapper around all three STAC object types.
//! This object contains the deserialized STAC object, as well as the href from which the object was read:
//!
//! ```
//! # use stac::{Reader, Read};
//! let reader = Reader::default();
//! let object = reader.read("data/catalog.json").unwrap();
//! assert_eq!(object.id(), "examples");
//! let catalog = object.as_catalog().unwrap();
//! assert_eq!(catalog.title.as_ref().unwrap(), "Example Catalog");
//! assert_eq!(object.href.as_ref().unwrap().as_str(), "data/catalog.json");
//! ```
//!
//! There is a top-level [read()] method for convenience:
//!
//! ```
//! let catalog = stac::read("data/catalog.json").unwrap();
//! ```
//!
//! The [Write] trait describes how to write any [Object] to an href.
//! The built-in [Writer] only knows how to write to the local filesystem -- writing to a url is an error:
//!
//! ```no_run
//! use stac::{Item, Object, Writer, Write};
//! let item = Item::new("an-id");
//! let object = Object::new(item, "item.json").unwrap();
//! let writer = Writer::default();
//! writer.write(object).unwrap();
//!
//! let item = Item::new("an-id");
//! let object = Object::new(item, "http://example.com/item.json").unwrap();
//! writer.write(object).unwrap_err();
//! ```
//!
//! # STAC catalogs are trees
//!
//! Because of Rust's strict mutability and ownership rules, tree structures require more verbose ergonomics than in other languages (e.g. Python in PySTAC).
//! The [Stac] structure is an arena-based tree inspired by [indextree](https://docs.rs/indextree/latest/indextree/).
//! The `Stac` arena uses handles to point to objects in the tree, providing an interface for interacting with a STAC catalog without relying on inner mutability.
//!
//! `Stac` can be created from an href.
//! The [read](Stac::read) method returns both the arena and a handle to the object:
//!
//! ```
//! use stac::Stac;
//! let (stac, handle) = Stac::read("data/catalog.json").unwrap();
//! ```
//!
//! A `Stac` is a lazy cache, meaning that it doesn't read objects until needed, and keeps read objects in a cache keyed by their hrefs.
//! Objects are read on-demand, e.g. via the [get](Stac::get) method:
//!
//! ```
//! # use stac::Stac;
//! let (mut stac, handle) = Stac::read("data/catalog.json").unwrap();
//! let child_handle = stac.children(handle).unwrap().next().unwrap();
//! let child = stac.get(child_handle).unwrap();
//! ```
//!
//! ## Rendering
//!
//! While STAC catalogs are trees, their organization via links is arbitrary.
//! 'Rendering' a STAC catalog is the process of creating hrefs and links to define its layout.
//! [Render::render] converts a [Stac] to an iterable of [Objects](Object).
//! The [BestPracticesRenderer] renders objects per the best practices as defined by the [STAC specification](https://github.com/radiantearth/stac-spec/blob/master/best-practices.md):
//!
//! ```
//! use stac::{Stac, Render, BestPracticesRenderer, Error};
//! let (mut stac, _) = Stac::read("data/catalog.json").unwrap();
//! let renderer = BestPracticesRenderer::new("the/root/directory").unwrap();
//! let objects = renderer.render(&mut stac).unwrap().collect::<Result<Vec<_>, Error>>().unwrap();
//! assert_eq!(objects.len(), 6);
//! assert_eq!(
//!     objects[0].href.as_ref().unwrap().as_str(),
//!     "the/root/directory/catalog.json"
//! );
//! ```
//!
//! ## Writing
//!
//! Stac trees have a [write](Stac::write) method that renders and writes all in one:
//!
//! ```no_run
//! use stac::{Stac, Render, BestPracticesRenderer, Writer};
//! let (mut stac, _) = Stac::read("data/catalog.json").unwrap();
//! let renderer = BestPracticesRenderer::new("the/root/directory").unwrap();
//! let writer = Writer::default();
//! stac.write(&renderer, &writer).unwrap();
//! ```
//!
//! # Full specification compliance
//!
//! The source repository contains canonical examples copied the [stac-spec repository](https://github.com/radiantearth/stac-spec/tree/master/examples), and these examples are tested for round trip equality.
//! For example:
//!
//! ```
//! use std::fs::File;
//! use std::io::BufReader;
//! use std::str::FromStr;
//! use serde_json::Value;
//! use stac::Item;
//!
//! let file = File::open("data/simple-item.json").unwrap();
//! let buf_reader = BufReader::new(file);
//! let before: Value = serde_json::from_reader(buf_reader).unwrap();
//! let item: Item = serde_json::from_value(before.clone()).unwrap();
//! let after = serde_json::to_value(item).unwrap();
//! assert_eq!(before, after);
//! ```

#![deny(
    elided_lifetimes_in_paths,
    explicit_outlives_requirements,
    keyword_idents,
    macro_use_extern_crate,
    meta_variable_misuse,
    missing_abi,
    missing_debug_implementations,
    missing_docs,
    non_ascii_idents,
    noop_method_call,
    pointer_structural_match,
    rust_2021_incompatible_closure_captures,
    rust_2021_incompatible_or_patterns,
    rust_2021_prefixes_incompatible_syntax,
    rust_2021_prelude_collisions,
    single_use_lifetimes,
    trivial_casts,
    trivial_numeric_casts,
    unreachable_pub,
    unsafe_code,
    unsafe_op_in_unsafe_fn,
    unused_crate_dependencies,
    unused_extern_crates,
    unused_import_braces,
    unused_lifetimes,
    unused_qualifications,
    unused_results
)]

mod asset;
mod catalog;
mod collection;
mod error;
mod extent;
mod href;
mod item;
mod link;
pub mod media_type;
mod object;
mod properties;
mod provider;
mod read;
mod render;
mod stac;
mod write;

pub use {
    crate::stac::{Handle, Handles, Items, Objects, Stac},
    asset::Asset,
    catalog::{Catalog, CATALOG_TYPE},
    collection::{Collection, COLLECTION_TYPE},
    error::Error,
    extent::{Extent, SpatialExtent, TemporalExtent},
    href::{Href, PathBufHref},
    item::{Item, ITEM_TYPE},
    link::Link,
    object::{Object, Value},
    properties::Properties,
    provider::Provider,
    read::{Read, Reader},
    render::{BestPracticesRenderer, Render},
    write::{Write, Writer},
};

/// The default STAC version supported by this library.
pub const STAC_VERSION: &str = "1.0.0";

/// Reads a STAC object from an href.
///
/// # Examples
///
/// ```
/// let catalog = stac::read("data/catalog.json").unwrap();
/// ```
pub fn read<T>(href: T) -> Result<Object, Error>
where
    T: Into<PathBufHref>,
{
    let reader = Reader::default();
    reader.read(href)
}

#[cfg(test)]
mod tests {
    macro_rules! roundtrip {
        ($function:ident, $filename:expr, $object:ident) => {
            #[test]
            fn $function() {
                use assert_json_diff::assert_json_eq;
                use serde_json::Value;
                use std::fs::File;
                use std::io::BufReader;

                let file = File::open($filename).unwrap();
                let buf_reader = BufReader::new(file);
                let before: Value = serde_json::from_reader(buf_reader).unwrap();
                let object: $object = serde_json::from_value(before.clone()).unwrap();
                let after = serde_json::to_value(object).unwrap();
                assert_json_eq!(before, after);
            }
        };
    }
    pub(crate) use roundtrip;
}

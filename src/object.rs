use crate::{
    catalog::{Catalog, CATALOG_TYPE},
    collection::{Collection, COLLECTION_TYPE},
    item::{Item, ITEM_TYPE},
    Error,
};
use serde_json::Value;
use std::convert::TryFrom;

/// An enum that can hold all three STAC object types.
#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Object {
    /// A STAC Catalog.
    Catalog(Catalog),

    /// A STAC Collection.
    Collection(Collection),

    /// A STAC Item.
    Item(Item),
}

impl Object {
    /// Creates a new Object from some JSON and an href.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::fs::File;
    /// use std::io::BufReader;
    /// use stac::Object;
    /// use serde_json::Value;
    ///
    /// let file = File::open("data/simple-item.json").unwrap();
    /// let buf_reader = BufReader::new(file);
    /// let value: Value = serde_json::from_reader(buf_reader).unwrap();
    /// let object = Object::new(value, "data/simple-item.json").unwrap();
    /// ```
    pub fn new<S: ToString>(value: Value, href: S) -> Result<Object, Error> {
        let mut object = Object::try_from(value)?;
        object.set_href(href);
        Ok(object)
    }

    /// Returns true if this object is a catalog.
    ///
    /// # Examples
    ///
    /// TODO
    pub fn is_catalog(&self) -> bool {
        matches!(self, Object::Catalog(_))
    }

    /// Returns true if this object is a collection.
    ///
    /// # Examples
    ///
    /// TODO
    pub fn is_collection(&self) -> bool {
        matches!(self, Object::Collection(_))
    }

    /// Returns true if this object is an item.
    ///
    /// # Examples
    ///
    /// TODO
    pub fn is_item(&self) -> bool {
        matches!(self, Object::Item(_))
    }

    /// Returns this object's href.
    ///
    /// # Examples
    ///
    /// TODO
    pub fn href(&self) -> Option<&str> {
        use Object::*;
        match self {
            Catalog(catalog) => catalog.href.as_deref(),
            Collection(collection) => collection.href.as_deref(),
            Item(item) => item.href.as_deref(),
        }
    }

    /// Sets this object's href.
    ///
    /// # Examples
    ///
    /// TODO
    pub fn set_href<S: ToString>(&mut self, href: S) {
        use Object::*;
        match self {
            Catalog(catalog) => catalog.href = Some(href.to_string()),
            Collection(collection) => collection.href = Some(href.to_string()),
            Item(item) => item.href = Some(href.to_string()),
        }
    }

    /// Returns true if this object has items.
    ///
    /// # Examples
    ///
    /// TODO
    pub fn has_items(&self) -> bool {
        unimplemented!()
    }

    /// Returns this object's id.
    ///
    /// # Examples
    ///
    /// TODO
    pub fn id(&self) -> &str {
        unimplemented!()
    }
}

impl TryFrom<Value> for Object {
    type Error = Error;

    fn try_from(mut value: Value) -> Result<Object, Error> {
        match value.get_mut("type") {
            Some(type_) => match type_.as_str() {
                Some(CATALOG_TYPE) => Ok(Object::Catalog(serde_json::from_value(value)?)),
                Some(COLLECTION_TYPE) => Ok(Object::Collection(serde_json::from_value(value)?)),
                Some(ITEM_TYPE) => Ok(Object::Item(serde_json::from_value(value)?)),
                Some(other) => Err(Error::InvalidTypeValue(other.to_owned())),
                None => Err(Error::InvalidTypeField(type_.take())),
            },
            None => Err(Error::MissingType),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Object;
    use serde_json::{json, Value};
    use std::{convert::TryFrom, fs::File, io::BufReader};

    fn from_path(path: &str) -> Value {
        let file = File::open(path).unwrap();
        let buf_reader = BufReader::new(file);
        serde_json::from_reader(buf_reader).unwrap()
    }

    #[test]
    fn new_catalog() {
        let value = from_path("data/catalog.json");
        let object = Object::new(value, "data/catalog.json").unwrap();
        assert!(object.is_catalog());
        assert_eq!(object.href().unwrap(), "data/catalog.json");
    }

    #[test]
    fn new_collection() {
        let value = from_path("data/collection.json");
        let object = Object::new(value, "data/collection.json").unwrap();
        assert!(object.is_collection());
        assert_eq!(object.href().unwrap(), "data/collection.json");
    }

    #[test]
    fn new_item() {
        let value = from_path("data/simple-item.json");
        let object = Object::new(value, "data/simple-item.json").unwrap();
        assert!(object.is_item());
        assert_eq!(object.href().unwrap(), "data/simple-item.json");
    }

    #[test]
    fn try_from() {
        assert!(Object::try_from(json!({})).is_err());
        assert!(Object::try_from(json!({
            "type": []
        }))
        .is_err(),);
        assert!(Object::try_from(json!({
            "type": "panda"
        }))
        .is_err(),);
    }
}

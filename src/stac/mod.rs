//! Arena-based tree for working with STAC catalogs.
//!
//! Because of Rust's strict mutability and ownership rules, tree structures require more verbose ergonomics than in other languages.
//! [Stac] is an arena-based tree inspired by [indextree](https://docs.rs/indextree/latest/indextree/).
//! The `Stac` arena uses [Handles](Handle) to point to objects in the tree.
//!
//! # Examples
//!
//! A `Stac` can be created from an href or an object.
//! When you create a `Stac`, you get back the `Stac` and a [Handle] to that object:
//!
//! ```
//! use stac::{Stac, Catalog};
//! let (stac, root) = Stac::new(Catalog::new("root")).unwrap();
//! // Stac::read uses `Reader` under the hood.
//! let (stac, root) = Stac::read("data/catalog.json").unwrap();
//! ```
//!
//! `Stac` is a lazy cache, meaning that it doesn't read objects until needed, and keeps read objects in a cache keyed by their hrefs.
//! Objects are read on-demand, e.g. via the [get](Stac::get) method, and any future access returns the stored object, instead of reading it again:
//!
//! ```
//! # use stac::Stac;
//! let (mut stac, root) = Stac::read("data/catalog.json").unwrap();
//! let children = stac.children(root); // <- none have the children have been read yet
//! let child = stac.get(children[0]).unwrap(); // <- the first child is read into the `Stac`
//! let child = stac.get(children[0]).unwrap(); // <- does not do any additional reads
//! ```
//!
//! Objects in a `Stac` may or may not have hrefs defined:
//!
//! ```
//! # use stac::{Stac, Catalog};
//! let (stac, root) = Stac::new(Catalog::new("root")).unwrap();
//! assert!(stac.href(root).is_none());
//! let (stac, root) = Stac::read("data/catalog.json").unwrap();
//! assert_eq!(stac.href(root).unwrap().as_str(), "data/catalog.json");
//! ```
//!
//! ## Walking
//!
//! Iterating over a STAC catalog is a useful for querying, modifying, or summarizing its contents.
//! The [walk] documentation describes how to walk over the objects in a [Stac].
//!
//! ```
//! # use stac::{Stac};
//! let (mut stac, root) = Stac::read("data/catalog.json").unwrap();
//! stac.walk(root)
//!     .visit(|stac, handle| stac.get(handle).map(|object| {
//!         println!("id: {}", object.id());
//!     }))
//!     .collect::<Result<Vec<_>, _>>()
//!     .unwrap();
//! ```
//!
//! ## Layout
//!
//! The structure of a STAC catalog is defined by its [Links](Link).
//! The process of translating a [Stac] tree into a set of `child`, `item`, `parent`, and `root` links is handled by [Layout].
//! The [layout](crate::layout) documentation describes how `Layouts` modify `Stacs`.
//!
//! ```
//! use stac::{Stac, Layout};
//! let (mut stac, root) = Stac::read("data/catalog.json").unwrap();
//! let mut layout = Layout::new("a/new/root");
//! layout.layout(&mut stac).unwrap();
//! assert_eq!(stac.href(root).unwrap().as_str(), "a/new/root/catalog.json");
//! ```
//!
//! ## Writing
//!
//! To avoid unnecessary copying, [Stac::write] consumes the `Stac` as it writes.
//! Each object is written to its `href`:
//!
//! ```no_run
//! use stac::{Stac, Layout, Writer, Write};
//! let (mut stac, _) = Stac::read("data/catalog.json").unwrap();
//! let mut layout = Layout::new("my/stac/v0");
//! let writer = Writer::default();
//! stac.write(&mut layout, &writer).unwrap();
//! ```

pub mod walk;

pub use walk::{BorrowedWalk, OwnedWalk, Walk};

use crate::{
    layout::Strategy, Error, Href, Layout, Link, Object, ObjectHrefTuple, Read, Reader, Result,
    Write,
};
use indexmap::IndexSet;
use std::collections::HashMap;

const ROOT_HANDLE: Handle = Handle(0);

/// An arena-based tree for working with STAC catalogs.
///
/// A `Stac` is generic over [Read], which allows `Stac`s to be configured to
/// use custom readers if needed. Many methods of `Stac` work with an
/// [ObjectHrefTuple], which is a tuple of an [Object] and an optional [Href].
/// [Object] and [HrefObject](crate::HrefObject) both implement [Into] for
/// [ObjectHrefTuple].
///
/// A `root` link is only used when creating a new `Stac`: if the initial object
/// has a `root` link, it is used to set the root of the `Stac`. After that, all
/// `root` links are ignored since the `root` is already set.
///
/// # Examples
///
/// ```
/// use stac::{Stac, Catalog, Href};
/// let catalog = Catalog::new("root");
/// let item = stac::read_item(&Href::new("data/simple-item.json")).unwrap();
/// let (mut stac, root) = Stac::new(catalog).unwrap();
/// let child = stac.add_child(root, item).unwrap();
/// ```
///
/// # Panics
///
/// A [Stac] uses [Handles](Handle) to reference objects in the tree. A `Handle`
/// is tied to its `Stac`; using a `Handle` on a `Stac` other than the one that
/// produced it is undefined behavior which may or may not panic.
///
/// TODO this should probably always panic.
///
#[derive(Debug)]
pub struct Stac<R: Read> {
    reader: R,
    nodes: Vec<Node>,
    free_nodes: Vec<Handle>,
    hrefs: HashMap<Href, Handle>,
}

/// A pointer to an [Object] in a [Stac] tree.
///
/// Handles can only be used on the `Stac` that produced them. Using a `Handle`
/// on a different `Stac` is undefined behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Handle(usize);

#[derive(Debug, Default)]
struct Node {
    object: Option<Object>,
    children: IndexSet<Handle>,
    parent: Option<Handle>,
    href: Option<Href>,
    is_from_item_link: bool,
}

impl Stac<Reader> {
    /// Creates a new `Stac` with the provided object and configured to use
    /// [Reader].
    ///
    /// Returns a tuple of the `Stac` and the [Handle] to the object.
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::{Stac, Catalog};
    /// let catalog = Catalog::new("an-id");
    /// let (stac, handle) = Stac::new(catalog).unwrap();
    /// ```
    pub fn new(object: impl Into<ObjectHrefTuple>) -> Result<(Stac<Reader>, Handle)> {
        Stac::new_with_reader(object, Reader::default())
    }

    /// Reads an [Href] with [Reader].
    ///
    /// Returns a tuple of the `Stac` and the [Handle] to the read [Object].
    ///
    /// # Examples
    ///
    /// ```
    /// use stac::Stac;
    /// let (stac, handle) = Stac::read("data/catalog.json").unwrap();
    /// ```
    pub fn read(href: impl Into<Href>) -> Result<(Stac<Reader>, Handle)> {
        let reader = Reader::default();
        let href_object = reader.read(href)?;
        Stac::new_with_reader(href_object, reader)
    }
}

impl<R: Read> Stac<R> {
    /// Creates a new `Stac` from the [Object] and [Read].
    ///
    /// Returns a tuple of the `Stac` and the [Handle] to that object.
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::{Stac, Reader};
    /// let catalog = stac::read("data/catalog.json").unwrap();
    /// let (stac, handle) = Stac::new_with_reader(catalog, Reader::default()).unwrap();
    /// ```
    pub fn new_with_reader(
        object: impl Into<ObjectHrefTuple>,
        reader: R,
    ) -> Result<(Stac<R>, Handle)> {
        let (object, href) = object.into();
        if let Some(link) = object.root_link() {
            let root_href = if let Some(href) = href.as_ref() {
                href.join(&link.href)?
            } else {
                link.href.clone().into()
            };
            if !href
                .as_ref()
                .map(|href| *href == root_href)
                .unwrap_or(false)
            {
                let root = reader.read(root_href)?;
                let (mut stac, _) = Stac::rooted(root, reader)?;
                let handle = stac.add(object)?;
                return Ok((stac, handle));
            }
        }
        Stac::rooted((object, href), reader)
    }

    fn rooted(object: impl Into<ObjectHrefTuple>, reader: R) -> Result<(Stac<R>, Handle)> {
        let handle = ROOT_HANDLE;
        let node = Node::default();
        let mut stac = Stac {
            reader,
            nodes: vec![node],
            free_nodes: Vec::new(),
            hrefs: HashMap::new(),
        };
        stac.set_object(handle, object)?;
        Ok((stac, handle))
    }

    /// Returns the root [Handle] of this `Stac`.
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::Stac;
    /// let (stac, root) = Stac::read("data/catalog.json").unwrap();
    /// assert_eq!(stac.root(), root);
    /// ```
    pub fn root(&self) -> Handle {
        ROOT_HANDLE
    }

    /// Returns a reference to an [Object] in this `Stac`.
    ///
    /// This method will resolve the object using its [Href], which requires a
    /// mutable reference to the `Stac`. This will return an [Err] if there is
    /// an error while reading the object.
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::Stac;
    /// let (mut stac, root) = Stac::read("data/catalog.json").unwrap();
    /// assert_eq!(stac.get(root).unwrap().id(), "examples");
    /// ```
    pub fn get(&mut self, handle: Handle) -> Result<&Object> {
        self.ensure_resolved(handle)?;
        Ok(self
            .node(handle)
            .object
            .as_ref()
            .expect("should be resolved"))
    }

    /// Returns the parent [Handle] of this object, if one is set.
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::Stac;
    /// let (mut stac, root) = Stac::read("data/catalog.json").unwrap();
    /// assert_eq!(stac.parent(root), None);
    /// let child = stac
    ///     .find(root, |object| object.id() == "extensions-collection")
    ///     .unwrap()
    ///     .unwrap();
    /// assert_eq!(stac.parent(child).unwrap(), root);
    /// ```
    pub fn parent(&self, handle: Handle) -> Option<Handle> {
        self.node(handle).parent
    }

    /// Adds an [Object] to the [Stac].
    ///
    /// If this object has links, the links will be resolved and the object will
    /// be linked into the tree.
    ///
    /// # Examples
    ///
    /// Adding an unattached object:
    ///
    /// ```
    /// # use stac::{Catalog, Stac};
    /// let (mut stac, root) = Stac::new(Catalog::new("a-catalog")).unwrap();
    /// let handle = stac.add(Catalog::new("unattached-catalog")).unwrap();
    /// ```
    ///
    /// Adding an object that will be linked into the tree:
    ///
    /// ```
    /// # use stac::{Catalog, HrefObject, Stac, Link};
    /// # let (mut stac, root) = Stac::new(Catalog::new("a-catalog")).unwrap();
    /// stac.set_href(root, "rootdir/catalog.json");
    /// let mut catalog = Catalog::new("attached-catalog");
    /// catalog.links.push(Link::parent("../catalog.json"));
    /// let href_object = HrefObject::new(catalog, "rootdir/attached-catalog/catalog.json");
    /// let child = stac.add(href_object).unwrap();
    /// assert_eq!(stac.parent(child).unwrap(), root);
    /// ```
    pub fn add(&mut self, object: impl Into<ObjectHrefTuple>) -> Result<Handle> {
        let (object, href) = object.into();
        let handle = href
            .as_ref()
            .and_then(|href| self.hrefs.get(&href).cloned())
            .unwrap_or_else(|| self.add_node());
        self.set_object(handle, (object, href))?;
        Ok(handle)
    }

    /// Adds an [Object] to the [Stac] as a child of the provided handle.
    ///
    /// If there is a `parent` link on the `Object`, it will be ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::{Item, Catalog, Link, Stac};
    /// let (mut stac, root) = Stac::new(Catalog::new("a-catalog")).unwrap();
    /// let child = stac.add_child(root, Item::new("an-item")).unwrap();
    /// assert_eq!(stac.parent(child).unwrap(), root);
    ///
    /// let mut second_item = Item::new("second-item");
    /// second_item.links.push(Link::parent("some/other/parent.json"));
    /// let child = stac.add_child(root, second_item).unwrap();
    /// assert_eq!(stac.parent(child).unwrap(), root);
    /// ```
    pub fn add_child(
        &mut self,
        parent: Handle,
        object: impl Into<ObjectHrefTuple>,
    ) -> Result<Handle> {
        let child = self.add(object)?;
        self.connect(parent, child);
        Ok(child)
    }

    /// Connects a parent and a child.
    ///
    /// This will disconnect the child from its current parent, if there is one.
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::{Stac, Catalog};
    /// let (mut stac, root) = Stac::new(Catalog::new("root")).unwrap();
    /// let disconnected = stac.add(Catalog::new("lonely")).unwrap();
    /// stac.connect(root, disconnected);
    /// assert_eq!(stac.parent(disconnected).unwrap(), root);
    /// ```
    pub fn connect(&mut self, parent: Handle, child: Handle) {
        if let Some(parent) = self.node(child).parent {
            if !self.node_mut(parent).children.remove(&child) {
                panic!("the child thought it had a parent but the parent didn't know about it");
            }
        }
        self.node_mut(child).parent = Some(parent);
        let _ = self.node_mut(parent).children.insert(child);
    }

    /// Removes an [Object] from the [Stac].
    ///
    /// Unlinks all parents and children. Note that this will leave the children
    /// unattached.  Returns the [Object] and its [Href], if they exist (one of
    /// them will). Returns an error if you try to remove the root object.
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::{Stac, Error};
    /// let (mut stac, root) = Stac::read("data/catalog.json").unwrap();
    /// let child = stac.find(root, |o| o.id() == "extensions-collection").unwrap().unwrap();
    /// let (child, href) = stac.remove(child).unwrap();
    /// assert_eq!(child.unwrap().id(), "extensions-collection");
    /// assert_eq!(href.unwrap().as_str(), "data/extensions-collection/collection.json");
    /// assert!(matches!(stac.remove(root).unwrap_err(), Error::CannotRemoveRoot));
    /// ```
    pub fn remove(&mut self, handle: Handle) -> Result<(Option<Object>, Option<Href>)> {
        if handle == self.root() {
            return Err(Error::CannotRemoveRoot);
        }
        let children = std::mem::take(&mut self.node_mut(handle).children);
        for child in children {
            self.disconnect(handle, child);
        }
        if let Some(parent) = self.node_mut(handle).parent.take() {
            self.disconnect(parent, handle);
        }
        let href = if let Some(href) = self.node_mut(handle).href.take() {
            let _ = self.hrefs.remove(&href);
            Some(href)
        } else {
            None
        };
        self.free_nodes.push(handle);
        let object = self.node_mut(handle).object.take();
        Ok((object, href))
    }

    /// Returns a vector of this object's children.
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::Stac;
    /// let (stac, root) = Stac::read("data/catalog.json").unwrap();
    /// let children = stac.children(root);
    /// ```
    pub fn children(&self, handle: Handle) -> Vec<Handle> {
        self.node(handle).children.iter().cloned().collect()
    }

    /// Returns the [Href] of an [Object].
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::{Stac, Catalog};
    /// let (mut stac, root) = Stac::read("data/catalog.json").unwrap();
    /// assert_eq!(stac.href(root).unwrap().as_str(), "data/catalog.json");
    /// let catalog = stac.add(Catalog::new("unattached")).unwrap();
    /// assert!(stac.href(catalog).is_none());
    /// ```
    pub fn href(&self, handle: Handle) -> Option<&Href> {
        self.node(handle).href.as_ref()
    }

    /// Sets the [Href] of an [Object].
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::{Catalog, Stac};
    /// let (mut stac, root) = Stac::new(Catalog::new("root")).unwrap();
    /// assert!(stac.href(root).is_none());
    /// stac.set_href(root, "path/to/the/root.catalog");
    /// assert_eq!(stac.href(root).unwrap().as_str(), "path/to/the/root.catalog");
    /// ```
    pub fn set_href(&mut self, handle: Handle, href: impl Into<Href>) {
        let href = href.into();
        let _ = self.hrefs.insert(href.clone(), handle);
        let _ = self.node_mut(handle).href.replace(href);
    }

    /// Finds an [Object] in the tree using a filter function.
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::Stac;
    /// let (mut stac, root) = Stac::read("data/catalog.json").unwrap();
    /// assert_eq!(stac.parent(root), None);
    /// let child = stac
    ///     .find(root, |object| object.id() == "extensions-collection")
    ///     .unwrap()
    ///     .unwrap();
    /// assert_eq!(stac.get(child).unwrap().id(), "extensions-collection");
    /// ```
    pub fn find(
        &mut self,
        handle: Handle,
        mut filter: impl FnMut(&Object) -> bool,
    ) -> Result<Option<Handle>> {
        self.walk(handle)
            .visit(|stac, handle| {
                let object = stac.get(handle)?;
                Ok((filter(object), handle))
            })
            .filter_map(|result| match result {
                Ok((keep, handle)) => {
                    if keep {
                        Some(Ok(handle))
                    } else {
                        None
                    }
                }
                Err(err) => Some(Err(err)),
            })
            .next()
            .transpose()
    }

    /// Adds a [Link] to an [Object].
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::{Stac, Catalog, Link};
    /// let (mut stac, root) = Stac::new(Catalog::new("an-id")).unwrap();
    /// stac.add_link(root, Link::new("an-href", "a-rel")).unwrap();
    /// ```
    pub fn add_link(&mut self, handle: Handle, link: Link) -> Result<()> {
        self.ensure_resolved(handle)?;
        self.node_mut(handle)
            .object
            .as_mut()
            .expect("resolved")
            .add_link(link);
        Ok(())
    }

    /// Takes the object out of the [Stac].
    ///
    /// # Examples
    ///
    /// ```
    /// # use stac::{Stac, Catalog};
    /// let catalog = Catalog::new("root");
    /// let (mut stac, root) = Stac::new(catalog.clone()).unwrap();
    /// assert_eq!(stac.take(root).unwrap().as_catalog().unwrap(), &catalog);
    /// ```
    pub fn take(&mut self, handle: Handle) -> Option<Object> {
        self.node_mut(handle).object.take()
    }

    /// Takes the [Href] from the [Object].
    pub fn take_href(&mut self, handle: Handle) -> Option<Href> {
        self.node_mut(handle).href.take()
    }

    /// Writes this [Stac], consuming it.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use stac::{Stac, Layout, Catalog, Writer, Write};
    /// let (stac, _) = Stac::new(Catalog::new("root")).unwrap();
    /// let mut layout = Layout::new("stac/v0");
    /// let writer = Writer::default();
    /// stac.write(&mut layout, &writer).unwrap();
    /// ```
    pub fn write<S>(self, layout: &mut Layout<S>, writer: &impl Write) -> Result<()>
    where
        S: Strategy,
    {
        for result in layout.render(self) {
            let href_object = result?;
            writer.write(href_object)?;
        }
        Ok(())
    }

    pub(crate) fn remove_structural_links(&mut self, handle: Handle) -> Result<()> {
        self.ensure_resolved(handle)?;
        self.node_mut(handle)
            .object
            .as_mut()
            .expect("resolved")
            .links_mut()
            .retain(|link| !link.is_structural());
        Ok(())
    }

    fn disconnect(&mut self, parent: Handle, child: Handle) {
        self.node_mut(child).parent = None;
        let _ = self.node_mut(parent).children.shift_remove(&child);
    }

    fn add_node(&mut self) -> Handle {
        if let Some(handle) = self.free_nodes.pop() {
            handle
        } else {
            let handle = Handle(self.nodes.len());
            self.nodes.push(Node::default());
            handle
        }
    }

    fn ensure_resolved(&mut self, handle: Handle) -> Result<()> {
        if self.node(handle).object.is_none() {
            if let Some(href) = self.node_mut(handle).href.take() {
                let href_object = self.reader.read(href)?;
                self.set_object(handle, href_object)?;
            } else {
                return Err(Error::UnresolvableNode);
            }
        }
        Ok(())
    }

    fn set_object(&mut self, handle: Handle, object: impl Into<ObjectHrefTuple>) -> Result<()> {
        let (object, href) = object.into();
        for link in object.links() {
            if !link.is_structural() {
                continue;
            }
            let other_href = if let Some(href) = href.as_ref() {
                href.join(&link.href)?
            } else {
                link.href.clone().into()
            };
            let other = if let Some(other) = self.hrefs.get(&other_href) {
                *other
            } else {
                let other = self.add_node();
                self.set_href(other, other_href);
                other
            };
            if link.is_child() || link.is_item() {
                if link.is_item() {
                    self.node_mut(other).is_from_item_link = true;
                }
                self.connect(handle, other);
            } else if link.is_parent() {
                // TODO what to do if there is already a parent?
                self.connect(other, handle);
            }
        }
        if let Some(href) = href {
            self.set_href(handle, href);
        } else {
            self.node_mut(handle).href = None;
        }
        let node = self.node_mut(handle);
        node.object = Some(object);
        Ok(())
    }

    fn is_item(&self, handle: Handle) -> bool {
        if let Some(object) = self.node(handle).object.as_ref() {
            object.is_item()
        } else {
            self.node(handle).is_from_item_link
        }
    }

    fn node(&self, handle: Handle) -> &Node {
        &self.nodes[handle.0]
    }

    fn node_mut(&mut self, handle: Handle) -> &mut Node {
        &mut self.nodes[handle.0]
    }
}

#[cfg(test)]
mod tests {
    use super::Stac;
    use crate::{Catalog, HrefObject, Item, Link};

    #[test]
    fn new() {
        let (mut stac, handle) = Stac::new(Catalog::new("an-id")).unwrap();
        assert_eq!(stac.get(handle).unwrap().id(), "an-id");
    }

    #[test]
    fn link() {
        let mut catalog = Catalog::new("an-id");
        catalog
            .links
            .push(Link::new("./subcatalog/catalog.json", "child"));
        let (mut stac, root_handle) =
            Stac::new(HrefObject::new(catalog, "a/path/catalog.json")).unwrap();
        let handle = stac
            .add(HrefObject::new(
                Catalog::new("child-catalog"),
                "a/path/subcatalog/catalog.json",
            ))
            .unwrap();
        assert_eq!(stac.parent(handle).unwrap(), root_handle);
    }

    #[test]
    fn add_child() {
        let (mut stac, root) = Stac::new(Catalog::new("an-id")).unwrap();
        let item = Item::new("an-id");
        let handle = stac.add_child(root, item).unwrap();
        assert_eq!(stac.parent(handle).unwrap(), root);
    }

    #[test]
    fn find_child() {
        let (mut stac, root) = Stac::read("data/catalog.json").unwrap();
        let child = stac
            .find(root, |object| object.id() == "extensions-collection")
            .unwrap()
            .unwrap();
        assert_eq!(stac.get(child).unwrap().id(), "extensions-collection");
    }

    #[test]
    fn read() {
        let (mut stac, handle) = Stac::read("data/catalog.json").unwrap();
        let catalog = stac.get(handle).unwrap();
        assert_eq!(catalog.id(), "examples");
    }

    #[test]
    fn read_non_root() {
        let (mut stac, handle) = Stac::read("data/extensions-collection/collection.json").unwrap();
        assert_eq!(stac.get(handle).unwrap().id(), "extensions-collection");
        assert_eq!(stac.get(stac.root()).unwrap().id(), "examples");
    }

    #[test]
    fn remove_returns_same_object() {
        let (mut stac, root) = Stac::new(Catalog::new("root")).unwrap();
        let mut child = Catalog::new("child");
        child.links.push(Link::root("../catalog.json"));
        child.links.push(Link::parent("../catalog.json"));
        child.links.push(Link::child("./subcatalog/catlog.json"));
        child.links.push(Link::item("./42/42.json"));
        let handle = stac.add_child(root, child.clone()).unwrap();
        assert_eq!(
            *stac
                .remove(handle)
                .unwrap()
                .0
                .unwrap()
                .as_catalog()
                .unwrap(),
            child
        );
    }

    #[test]
    fn disconnect_old_parent() {
        let (mut stac, root) = Stac::new(Catalog::new("root")).unwrap();
        let child1 = stac.add_child(root, Catalog::new("child1")).unwrap();
        let child2 = stac.add_child(root, Catalog::new("child2")).unwrap();
        stac.connect(child1, child2);
        assert_eq!(stac.children(root).len(), 1);
    }
}

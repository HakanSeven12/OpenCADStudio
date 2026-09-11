//! Group object - Named collection of entities

use crate::types::Handle;

/// Group object - represents a named group of entities
///
/// Groups are used to organize entities into logical collections.
/// Entities in a group can be selected and manipulated together.
///
/// # DXF Object Type
/// GROUP
///
/// # Example
/// ```ignore
/// use acadrust::objects::Group;
/// use acadrust::types::Handle;
///
/// let mut group = Group::new("MyGroup");
/// group.description = "A collection of related entities".to_string();
/// group.add_entity(Handle::new(100));
/// group.add_entity(Handle::new(101));
/// ```
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Group {
    /// Unique handle
    pub handle: Handle,
    /// Owner handle (typically a dictionary)
    pub owner: Handle,
    /// Group name
    pub name: String,
    /// DWG/DXF unnamed flag. This is stored independently from the dictionary
    /// key and must not be inferred after reading a binary drawing.
    #[cfg_attr(feature = "serde", serde(default))]
    pub unnamed: bool,
    /// Group description (DXF code 300)
    pub description: String,
    /// Entity handles in the group (DXF code 340, hard-pointer)
    pub entities: Vec<Handle>,
    /// Group is selectable (DXF code 71, default: true)
    pub selectable: bool,
}

impl Group {
    /// Object type name
    pub const OBJECT_TYPE: &'static str = "GROUP";

    /// Create a new named group
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            handle: Handle::NULL,
            owner: Handle::NULL,
            name: name.into(),
            unnamed: false,
            description: String::new(),
            entities: Vec::new(),
            selectable: true,
        }
    }

    /// Create an unnamed group (name starts with "*")
    pub fn unnamed() -> Self {
        let mut group = Self::new("*A");
        group.unnamed = true;
        group
    }

    /// Create a group with description
    pub fn with_description(name: impl Into<String>, description: impl Into<String>) -> Self {
        let mut group = Self::new(name);
        group.description = description.into();
        group
    }

    /// Check if this is an unnamed group
    /// Unnamed groups have empty names or names starting with "*"
    pub fn is_unnamed(&self) -> bool {
        self.unnamed
    }

    /// Add an entity to the group
    pub fn add_entity(&mut self, handle: Handle) {
        if !self.entities.contains(&handle) {
            self.entities.push(handle);
        }
    }

    /// Add multiple entities to the group
    pub fn add_entities(&mut self, handles: impl IntoIterator<Item = Handle>) {
        for handle in handles {
            self.add_entity(handle);
        }
    }

    /// Remove an entity from the group
    pub fn remove_entity(&mut self, handle: Handle) -> bool {
        if let Some(pos) = self.entities.iter().position(|h| *h == handle) {
            self.entities.remove(pos);
            true
        } else {
            false
        }
    }

    /// Clear all entities from the group
    pub fn clear(&mut self) {
        self.entities.clear();
    }

    /// Get the number of entities in the group
    pub fn len(&self) -> usize {
        self.entities.len()
    }

    /// Check if the group is empty
    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Check if the group contains a specific entity
    pub fn contains(&self, handle: Handle) -> bool {
        self.entities.contains(&handle)
    }

    /// Get an entity handle by index
    pub fn get(&self, index: usize) -> Option<Handle> {
        self.entities.get(index).copied()
    }

    /// Iterate over entity handles
    pub fn iter(&self) -> impl Iterator<Item = &Handle> {
        self.entities.iter()
    }

    /// Set the group as selectable or not
    pub fn set_selectable(&mut self, selectable: bool) {
        self.selectable = selectable;
    }

    /// Builder: Set description
    pub fn with_desc(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Builder: Set selectable
    pub fn with_selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// Builder: Add entity
    pub fn with_entity(mut self, handle: Handle) -> Self {
        self.add_entity(handle);
        self
    }

    /// Builder: Add multiple entities
    pub fn with_entities(mut self, handles: impl IntoIterator<Item = Handle>) -> Self {
        self.add_entities(handles);
        self
    }
}

impl Default for Group {
    fn default() -> Self {
        Self::unnamed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_group_creation() {
        let group = Group::new("TestGroup");
        assert_eq!(group.name, "TestGroup");
        assert!(group.is_empty());
        assert!(group.selectable);
        assert!(!group.is_unnamed());
    }

    #[test]
    fn test_group_unnamed() {
        let group = Group::unnamed();
        assert!(group.is_unnamed());
        assert!(group.name.starts_with('*'));
    }

    #[test]
    fn test_group_with_description() {
        let group = Group::with_description("MyGroup", "This is a test group");
        assert_eq!(group.name, "MyGroup");
        assert_eq!(group.description, "This is a test group");
    }

    #[test]
    fn test_group_add_entity() {
        let mut group = Group::new("Test");
        group.add_entity(Handle::new(100));
        group.add_entity(Handle::new(101));

        assert_eq!(group.len(), 2);
        assert!(group.contains(Handle::new(100)));
        assert!(group.contains(Handle::new(101)));
    }

    #[test]
    fn test_group_add_duplicate() {
        let mut group = Group::new("Test");
        group.add_entity(Handle::new(100));
        group.add_entity(Handle::new(100)); // Duplicate

        assert_eq!(group.len(), 1);
    }

    #[test]
    fn test_group_add_entities() {
        let mut group = Group::new("Test");
        group.add_entities(vec![Handle::new(100), Handle::new(101), Handle::new(102)]);

        assert_eq!(group.len(), 3);
    }

    #[test]
    fn test_group_remove_entity() {
        let mut group = Group::new("Test");
        group.add_entity(Handle::new(100));
        group.add_entity(Handle::new(101));

        assert!(group.remove_entity(Handle::new(100)));
        assert_eq!(group.len(), 1);
        assert!(!group.contains(Handle::new(100)));

        assert!(!group.remove_entity(Handle::new(999))); // Not found
    }

    #[test]
    fn test_group_clear() {
        let mut group = Group::new("Test");
        group.add_entities(vec![Handle::new(100), Handle::new(101)]);

        group.clear();
        assert!(group.is_empty());
    }

    #[test]
    fn test_group_get() {
        let mut group = Group::new("Test");
        group.add_entity(Handle::new(100));
        group.add_entity(Handle::new(101));

        assert_eq!(group.get(0), Some(Handle::new(100)));
        assert_eq!(group.get(1), Some(Handle::new(101)));
        assert_eq!(group.get(2), None);
    }

    #[test]
    fn test_group_selectable() {
        let mut group = Group::new("Test");
        assert!(group.selectable);

        group.set_selectable(false);
        assert!(!group.selectable);
    }

    #[test]
    fn test_group_builder() {
        let group = Group::new("Test")
            .with_desc("Description")
            .with_selectable(false)
            .with_entity(Handle::new(100))
            .with_entities(vec![Handle::new(101), Handle::new(102)]);

        assert_eq!(group.description, "Description");
        assert!(!group.selectable);
        assert_eq!(group.len(), 3);
    }

    #[test]
    fn test_group_iter() {
        let mut group = Group::new("Test");
        group.add_entities(vec![Handle::new(100), Handle::new(101)]);

        let handles: Vec<Handle> = group.iter().copied().collect();
        assert_eq!(handles, vec![Handle::new(100), Handle::new(101)]);
    }
}

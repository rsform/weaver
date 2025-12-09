#![cfg(feature = "iroh")]

//! Presence tracking for collaborative editing sessions.
//!
//! Tracks active collaborators, their cursor positions, and display info.

use std::collections::HashMap;

use iroh::EndpointId;
use web_time::Instant;

/// A remote collaborator's cursor state.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteCursor {
    /// Character offset in the document.
    pub position: usize,
    /// Selection range (anchor, head) if any.
    pub selection: Option<(usize, usize)>,
    /// Assigned colour (RGBA).
    pub color: u32,
    /// When this cursor was last updated.
    pub updated_at: Instant,
}

/// A collaborator in the session.
#[derive(Debug, Clone)]
pub struct Collaborator {
    /// The collaborator's DID.
    pub did: String,
    /// Display name for UI.
    pub display_name: String,
    /// Assigned colour (RGBA).
    pub color: u32,
    /// Current cursor state.
    pub cursor: Option<RemoteCursor>,
    /// iroh endpoint ID for this peer.
    pub node_id: EndpointId,
}

/// Tracks all collaborators in a session.
#[derive(Debug, Default, Clone)]
pub struct PresenceTracker {
    /// Collaborators by EndpointId.
    collaborators: HashMap<EndpointId, Collaborator>,
    /// Colour assignment counter.
    next_color_index: usize,
}

/// Predefined collaborator colours (pastel-ish for readability).
const COLLABORATOR_COLORS: [u32; 8] = [
    0xFF6B6BFF, // Red
    0x4ECDC4FF, // Teal
    0xFFE66DFF, // Yellow
    0x95E1D3FF, // Mint
    0xF38181FF, // Coral
    0xAA96DAFF, // Purple
    0xFCBF49FF, // Orange
    0x2EC4B6FF, // Cyan
];

impl PresenceTracker {
    /// Create a new presence tracker.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a collaborator when they join.
    pub fn add_collaborator(&mut self, node_id: EndpointId, did: String, display_name: String) {
        let color = self.assign_color();
        self.collaborators.insert(
            node_id,
            Collaborator {
                did,
                display_name,
                color,
                cursor: None,
                node_id,
            },
        );
    }

    /// Remove a collaborator when they leave.
    pub fn remove_collaborator(&mut self, node_id: &EndpointId) -> Option<Collaborator> {
        self.collaborators.remove(node_id)
    }

    /// Update a collaborator's cursor position.
    pub fn update_cursor(
        &mut self,
        node_id: &EndpointId,
        position: usize,
        selection: Option<(usize, usize)>,
    ) {
        if let Some(collab) = self.collaborators.get_mut(node_id) {
            collab.cursor = Some(RemoteCursor {
                position,
                selection,
                color: collab.color,
                updated_at: Instant::now(),
            });
        }
    }

    /// Get all active collaborators.
    pub fn collaborators(&self) -> impl Iterator<Item = &Collaborator> {
        self.collaborators.values()
    }

    /// Get all remote cursors (for rendering).
    pub fn cursors(&self) -> impl Iterator<Item = (&Collaborator, &RemoteCursor)> {
        self.collaborators
            .values()
            .filter_map(|c| c.cursor.as_ref().map(|cursor| (c, cursor)))
    }

    /// Get a collaborator by EndpointId.
    pub fn get(&self, node_id: &EndpointId) -> Option<&Collaborator> {
        self.collaborators.get(node_id)
    }

    /// Check if an EndpointId is a known collaborator.
    pub fn contains(&self, node_id: &EndpointId) -> bool {
        self.collaborators.contains_key(node_id)
    }

    /// Number of active collaborators.
    pub fn len(&self) -> usize {
        self.collaborators.len()
    }

    /// Check if there are no collaborators.
    pub fn is_empty(&self) -> bool {
        self.collaborators.is_empty()
    }

    /// Assign a colour to a new collaborator.
    fn assign_color(&mut self) -> u32 {
        let color = COLLABORATOR_COLORS[self.next_color_index % COLLABORATOR_COLORS.len()];
        self.next_color_index += 1;
        color
    }

    /// Remove stale cursors that haven't been updated recently.
    pub fn prune_stale_cursors(&mut self, max_age: std::time::Duration) {
        let now = Instant::now();
        for collab in self.collaborators.values_mut() {
            if let Some(ref cursor) = collab.cursor {
                if now.duration_since(cursor.updated_at) > max_age {
                    collab.cursor = None;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_node_id() -> EndpointId {
        use iroh::SecretKey;
        SecretKey::generate(&mut rand::rng()).public()
    }

    #[test]
    fn test_add_remove_collaborator() {
        let mut tracker = PresenceTracker::new();
        let node_id = test_node_id();

        tracker.add_collaborator(node_id, "did:plc:test".into(), "Alice".into());
        assert_eq!(tracker.len(), 1);
        assert!(tracker.contains(&node_id));

        let removed = tracker.remove_collaborator(&node_id);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().display_name, "Alice");
        assert!(tracker.is_empty());
    }

    #[test]
    fn test_cursor_update() {
        let mut tracker = PresenceTracker::new();
        let node_id = test_node_id();

        tracker.add_collaborator(node_id, "did:plc:test".into(), "Bob".into());
        tracker.update_cursor(&node_id, 42, Some((40, 50)));

        let collab = tracker.get(&node_id).unwrap();
        let cursor = collab.cursor.as_ref().unwrap();
        assert_eq!(cursor.position, 42);
        assert_eq!(cursor.selection, Some((40, 50)));
    }

    #[test]
    fn test_color_assignment() {
        let mut tracker = PresenceTracker::new();

        for i in 0..10 {
            let node_id = test_node_id();
            tracker.add_collaborator(node_id, format!("did:plc:test{}", i), format!("User{}", i));
        }

        // Each collaborator should have a colour
        for collab in tracker.collaborators() {
            assert!(collab.color != 0);
        }
    }
}

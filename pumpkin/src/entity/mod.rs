use std::sync::{atomic::AtomicBool, Arc, Mutex};

use num_derive::{FromPrimitive, ToPrimitive};
use num_traits::ToPrimitive;
use pumpkin_core::math::{
    get_section_cord, position::WorldPosition, vector2::Vector2, vector3::Vector3,
};
use pumpkin_entity::{entity_type::EntityType, pose::EntityPose, EntityId};
use pumpkin_protocol::{
    client::play::{CSetEntityMetadata, Metadata},
    VarInt,
};

use crate::world::World;

pub mod player;

pub struct Entity {
    /// A unique identifier for the entity
    pub entity_id: EntityId,
    /// The type of entity (e.g., player, zombie, item)
    pub entity_type: EntityType,
    /// The world in which the entity exists.
    pub world: Arc<World>,
    /// The entity's current health level.
    pub health: Mutex<f32>,

    /// The entity's current position in the world
    pub pos: Mutex<Vector3<f64>>,
    /// The entity's position rounded to the nearest block coordinates
    pub block_pos: Mutex<WorldPosition>,
    /// The chunk coordinates of the entity's current position
    pub chunk_pos: Mutex<Vector2<i32>>,

    /// Indicates whether the entity is sneaking
    pub sneaking: AtomicBool,
    /// Indicates whether the entity is sprinting
    pub sprinting: AtomicBool,
    /// Indicates whether the entity is flying due to a fall
    pub fall_flying: AtomicBool,
    /// The entity's current velocity vector, aka Knockback
    pub velocity: Mutex<Vector3<f64>>,

    /// Indicates whether the entity is on the ground (may not always be accurate).
    pub on_ground: AtomicBool,

    /// The entity's yaw rotation (horizontal rotation) ← →
    pub yaw: Mutex<f32>,
    /// The entity's head yaw rotation (horizontal rotation of the head)
    pub head_yaw: Mutex<f32>,
    /// The entity's pitch rotation (vertical rotation) ↑ ↓
    pub pitch: Mutex<f32>,
    /// The height of the entity's eyes from the ground.
    // TODO: Change this in diffrent poses
    pub standing_eye_height: f32,
    /// The entity's current pose (e.g., standing, sitting, swimming).
    pub pose: Mutex<EntityPose>,
}

impl Entity {
    pub fn new(
        entity_id: EntityId,
        world: Arc<World>,
        entity_type: EntityType,
        standing_eye_height: f32,
    ) -> Self {
        Self {
            entity_id,
            entity_type,
            on_ground: AtomicBool::new(false),
            pos: Mutex::new(Vector3::new(0.0, 0.0, 0.0)),
            block_pos: Mutex::new(WorldPosition(Vector3::new(0, 0, 0))),
            chunk_pos: Mutex::new(Vector2::new(0, 0)),
            sneaking: AtomicBool::new(false),
            world,
            // TODO: Load this from previous instance
            health: Mutex::new(20.0),
            sprinting: AtomicBool::new(false),
            fall_flying: AtomicBool::new(false),
            yaw: Mutex::new(0.0),
            head_yaw: Mutex::new(0.0),
            pitch: Mutex::new(0.0),
            velocity: Mutex::new(Vector3::new(0.0, 0.0, 0.0)),
            standing_eye_height,
            pose: Mutex::new(EntityPose::Standing),
        }
    }

    /// Updates the entity's position, block position, and chunk position.
    ///
    /// This function calculates the new position, block position, and chunk position based on the provided coordinates. If any of these values change, the corresponding fields are updated.
    pub fn set_pos(&self, x: f64, y: f64, z: f64) {
        let mut pos = self.pos.lock().unwrap();
        if pos.x != x || pos.y != y || pos.z != z {
            *pos = Vector3::new(x, y, z);
            let i = x.floor() as i32;
            let j = y.floor() as i32;
            let k = z.floor() as i32;

            let mut block_pos = self.block_pos.lock().unwrap();
            let block_pos_vec = block_pos.0;
            if i != block_pos_vec.x || j != block_pos_vec.y || k != block_pos_vec.z {
                *block_pos = WorldPosition(Vector3::new(i, j, k));

                let mut chunk_pos = self.chunk_pos.lock().unwrap();
                if get_section_cord(i) != chunk_pos.x || get_section_cord(k) != chunk_pos.z {
                    *chunk_pos = Vector2::new(
                        get_section_cord(block_pos_vec.x),
                        get_section_cord(block_pos_vec.z),
                    );
                }
            }
        }
    }

    /// Sets the Entity yaw & pitch Rotation
    pub fn set_rotation(&self, yaw: f32, pitch: f32) {
        // TODO
        *self.yaw.lock().unwrap() = yaw;
        *self.pitch.lock().unwrap() = pitch
    }

    /// Removes the Entity from their current World
    pub async fn remove(&mut self) {
        self.world.remove_entity(self);
    }

    /// Applies knockback to the entity, following vanilla Minecraft's mechanics.
    ///
    /// This function calculates the entity's new velocity based on the specified knockback strength and direction.
    pub fn knockback(&self, strength: f64, x: f64, z: f64) {
        // This has some vanilla magic
        let mut x = x;
        let mut z = z;
        while x * x + z * z < 1.0E-5 {
            x = (rand::random::<f64>() - rand::random::<f64>()) * 0.01;
            z = (rand::random::<f64>() - rand::random::<f64>()) * 0.01;
        }

        let var8 = Vector3::new(x, 0.0, z).normalize() * strength;
        let mut velocity = self.velocity.lock().unwrap();
        *velocity = Vector3::new(
            velocity.x / 2.0 - var8.x,
            if self.on_ground.load(std::sync::atomic::Ordering::Relaxed) {
                (velocity.y / 2.0 + strength).min(0.4)
            } else {
                velocity.y
            },
            velocity.z / 2.0 - var8.z,
        );
    }

    pub async fn set_sneaking(&self, sneaking: bool) {
        assert!(self.sneaking.load(std::sync::atomic::Ordering::Relaxed) != sneaking);
        self.sneaking
            .store(sneaking, std::sync::atomic::Ordering::Relaxed);
        self.set_flag(Flag::Sneaking, sneaking).await;
        // if sneaking {
        //     self.set_pose(EntityPose::Crouching).await;
        // } else {
        //     self.set_pose(EntityPose::Standing).await;
        // }
    }

    pub async fn set_sprinting(&self, sprinting: bool) {
        assert!(self.sprinting.load(std::sync::atomic::Ordering::Relaxed) != sprinting);
        self.sprinting
            .store(sprinting, std::sync::atomic::Ordering::Relaxed);
        self.set_flag(Flag::Sprinting, sprinting).await;
    }

    pub fn check_fall_flying(&self) -> bool {
        !self.on_ground.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub async fn set_fall_flying(&self, fall_flying: bool) {
        assert!(self.fall_flying.load(std::sync::atomic::Ordering::Relaxed) != fall_flying);
        self.fall_flying
            .store(fall_flying, std::sync::atomic::Ordering::Relaxed);
        self.set_flag(Flag::FallFlying, fall_flying).await;
    }

    async fn set_flag(&self, flag: Flag, value: bool) {
        let index = flag.to_u32().unwrap();
        let mut b = 0i8;
        if value {
            b |= 1 << index;
        } else {
            b &= !(1 << index);
        }
        let packet = CSetEntityMetadata::new(self.entity_id.into(), Metadata::new(0, 0.into(), b));
        self.world.broadcast_packet_all(&packet);
    }

    pub async fn set_pose(&self, pose: EntityPose) {
        *self.pose.lock().unwrap() = pose;
        let pose = pose as i32;
        let packet = CSetEntityMetadata::<VarInt>::new(
            self.entity_id.into(),
            Metadata::new(6, 20.into(), (pose).into()),
        );
        self.world.broadcast_packet_all(&packet)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, FromPrimitive, ToPrimitive)]
/// Represents various entity flags that are sent in entity metadata.
///
/// These flags are used by the client to modify the rendering of entities based on their current state.
///
/// **Purpose:**
///
/// This enum provides a more type-safe and readable way to represent entity flags compared to using raw integer values.
pub enum Flag {
    /// Indicates if the entity is on fire.
    OnFire,
    /// Indicates if the entity is sneaking.
    Sneaking,
    /// Indicates if the entity is sprinting.
    Sprinting,
    /// Indicates if the entity is swimming.
    Swimming,
    /// Indicates if the entity is invisible.
    Invisible,
    /// Indicates if the entity is glowing.
    Glowing,
    /// Indicates if the entity is flying due to a fall.
    FallFlying,
}

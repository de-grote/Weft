//! What resource packs change of block and item shapes: a pack's `models/block/<name>.json` or
//! `models/item/<name>.json` redraws that block or item. Only the drawing: collision, selection,
//! light and which neighbors a block hides stay what the mod gave every player, since the server
//! knows nothing of packs.

use std::sync::{Arc, RwLock};

use rustc_hash::FxHashMap;

use crate::assets::Assets;
use crate::block_model::{self, FileModel};
use crate::content::{Content, ModelBox, StateInfo};
use weft_abi::BlockModel;

#[derive(Default)]
pub struct StateLook {
    /// Drawn instead of the state's own shape.
    pub boxes: Option<Vec<ModelBox>>,
    /// A cube parent's face textures.
    pub textures: Option<[u32; 6]>,
}

#[derive(Default)]
pub struct ItemLook {
    pub icon: Option<u32>,
    pub model: Option<Vec<ModelBox>>,
}

#[derive(Default)]
pub struct Looks {
    /// Per state, up to the last with a look.
    states: Vec<Option<StateLook>>,
    items: FxHashMap<u32, ItemLook>,
    /// Texture names beyond the content's, by id after them.
    names: Vec<String>,
}

/// What a state is drawn as, a pack's model counted: all that goes by what a block shows asks here.
pub struct Drawn<'a> {
    pub model: BlockModel,
    pub boxes: &'a [ModelBox],
    pub textures: [u32; 6],
}

impl Drawn<'_> {
    /// The textures it shows on a face. A plant has one face, the first.
    pub fn on_face(&self, dir: usize) -> Vec<u32> {
        match self.model {
            BlockModel::Boxes => self
                .boxes
                .iter()
                .filter(|b| super::mesher::box_face(b, dir).is_some())
                .map(|b| b.textures[dir])
                .collect(),
            BlockModel::Cube => vec![self.textures[dir]],
            BlockModel::Cross if dir == 0 => vec![self.textures[0]],
            _ => Vec::new(),
        }
    }
}

/// What an item is drawn as, in a slot and in the hand alike.
pub enum ItemDraw<'a> {
    /// Its own boxes.
    Model(&'a [ModelBox]),
    /// A sprite.
    Icon(u32),
    /// As its block shows, `Looks::block_sprite` or else `block_shape`.
    Block(u32),
    Nothing,
}

impl Looks {
    pub fn state(&self, state: u32) -> Option<&StateLook> {
        self.states.get(state as usize)?.as_ref()
    }
    pub fn item(&self, item: u32) -> Option<&ItemLook> {
        self.items.get(&item)
    }

    pub fn drawn<'a>(&'a self, state: u32, s: &'a StateInfo) -> Drawn<'a> {
        let look = self.state(state);
        let boxes = look.and_then(|l| l.boxes.as_deref());
        Drawn {
            model: if boxes.is_some() { BlockModel::Boxes } else { s.model },
            boxes: boxes.unwrap_or(&s.boxes),
            textures: look.and_then(|l| l.textures).unwrap_or(s.textures),
        }
    }

    pub fn item_draw<'a>(&'a self, c: &'a Content, item: u32) -> ItemDraw<'a> {
        let Some(def) = c.items.get(item as usize).filter(|_| item != 0) else { return ItemDraw::Nothing };
        let look = self.item(item);
        let model = look.and_then(|l| l.model.as_deref()).unwrap_or(&def.model);
        let icon = look.and_then(|l| l.icon).unwrap_or(def.icon);
        match def.block {
            _ if !model.is_empty() => ItemDraw::Model(model),
            _ if icon != u32::MAX => ItemDraw::Icon(icon),
            Some(state) => ItemDraw::Block(state),
            None => ItemDraw::Nothing,
        }
    }

    /// The sprite a block's item shows if it is flat.
    pub fn block_sprite(&self, state: u32, s: &StateInfo) -> Option<u32> {
        let d = self.drawn(state, s);
        if d.model == BlockModel::Boxes {
            return None;
        }
        s.item_sprite()
            .map(|t| if s.liquid.is_none() { d.textures[0] } else { t })
    }

    /// The boxes a block's item shows if it isn't.
    pub fn block_shape(&self, state: u32, s: &StateInfo) -> Vec<ModelBox> {
        let d = self.drawn(state, s);
        if d.model == BlockModel::Boxes && !d.boxes.is_empty() { d.boxes.to_vec() } else { vec![ModelBox::whole(d.textures)] }
    }

    pub fn texture_name<'a>(&'a self, c: &'a Content, id: u32) -> Option<&'a str> {
        let id = id as usize;
        c.textures
            .get(id)
            .or_else(|| self.names.get(id.checked_sub(c.textures.len())?))
            .map(String::as_str)
    }
}

/// Every texture name there is, the content's first, numbering new ones.
pub struct Names {
    pub list: Vec<String>,
    ids: FxHashMap<String, u32>,
}

impl Names {
    pub fn id(&mut self, name: &str) -> u32 {
        if let Some(&i) = self.ids.get(name) {
            return i;
        }
        let i = self.list.len() as u32;
        self.list.push(name.to_string());
        self.ids.insert(name.to_string(), i);
        i
    }
}

/// What a model file draws, by texture ids. A face it names no texture for shows `own` of that
/// face. A cube parent is a whole cube's box.
pub fn file_boxes(m: &FileModel, own: &dyn Fn(usize) -> u32, id: &mut dyn FnMut(&str) -> u32) -> Vec<ModelBox> {
    let mut named = |t: &str, i: usize| if t.is_empty() { own(i) } else { id(t) };
    if let Some(cube) = m.cube.as_ref().filter(|_| m.boxes.is_empty()) {
        return vec![ModelBox::whole(std::array::from_fn(|i| named(&cube[i], i)))];
    }
    m.boxes.iter().map(|b| b.model_box(&mut named)).collect()
}

static CURRENT: RwLock<Option<Arc<Looks>>> = RwLock::new(None);

pub fn current() -> Arc<Looks> {
    CURRENT.read().unwrap().clone().unwrap_or_default()
}

pub fn set(looks: Looks) {
    *CURRENT.write().unwrap() = Some(Arc::new(looks));
}

/// The packs' models, and the names of all textures so far.
pub fn load(assets: &Assets, c: &Content) -> (Looks, Names) {
    let mut looks = Looks::default();
    let mut names = Names {
        list: c.textures.clone(),
        ids: c
            .textures
            .iter()
            .enumerate()
            .map(|(i, n)| (n.clone(), i as u32))
            .collect(),
    };
    let mut id = |name: &str| names.id(name);
    let read = |name: &str| {
        assets
            .read(name, "models", "json")
            .map(|b| String::from_utf8_lossy(&b).into_owned())
    };
    let from_pack = |name: &str| -> Option<FileModel> {
        assets.pack_path(name, "models", "json")?;
        block_model::load(name, &read).unwrap_or_else(|e| {
            log::warn!("resource pack {e}");
            None
        })
    };
    for b in &c.blocks {
        let Some((ns, path)) = b.name.split_once(':') else { continue };
        let Some(m) = from_pack(&format!("{ns}:block/{path}")) else { continue };
        for st in b.first_state..b.first_state + b.states {
            let s = &c.states[st as usize];
            // The file is written for the block unturned, as the mod's own model is.
            let t = s.turn;
            let own = |i: usize| s.textures[t.face(i)];
            let mut look = StateLook::default();
            match &m.cube {
                // Only a cube is kept one: it hides the faces against its neighbors. Anything
                // else is redrawn as a cube's box.
                Some(cube) if s.model == BlockModel::Cube => {
                    let own: [u32; 6] = std::array::from_fn(|i| if cube[i].is_empty() { own(i) } else { id(&cube[i]) });
                    look.textures = Some(crate::content::turned_faces(t, own));
                }
                _ => {
                    let boxes: Vec<ModelBox> = file_boxes(&m, &own, &mut id)
                        .iter()
                        .map(|b| b.turned(t))
                        .collect();
                    look.boxes = Some(boxes).filter(|b| !b.is_empty());
                }
            }
            if looks.states.len() <= st as usize {
                looks.states.resize_with(st as usize + 1, || None);
            }
            looks.states[st as usize] = Some(look);
        }
    }
    for (i, d) in c.items.iter().enumerate().skip(1) {
        let Some((ns, path)) = d.name.split_once(':') else { continue };
        let Some(m) = from_pack(&format!("{ns}:item/{path}")) else { continue };
        let icon = m.sprite.as_deref().filter(|t| !t.is_empty()).map(&mut id);
        let own = icon.unwrap_or(if d.icon == u32::MAX { 0 } else { d.icon });
        let model = (!m.boxes.is_empty()).then(|| file_boxes(&m, &|_| own, &mut id));
        looks.items.insert(i as u32, ItemLook { icon, model });
    }
    if !looks.states.is_empty() || !looks.items.is_empty() {
        log::info!("resource packs reshape {} block states and {} items", looks.states.iter().flatten().count(), looks.items.len());
    }
    looks.names = names.list[c.textures.len()..].to_vec();
    (looks, names)
}

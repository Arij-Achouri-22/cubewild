//! Inventory, crafting and equipment UI. Mutations are dispatched as actions.
use super::*;
impl Game {
    pub(super) fn draw_inventory(&self, mut ui: &mut Ui, font: &Font, cx: f32, _cy: f32, wf: f32, hf: f32, actions: &mut Vec<Action>) {
        let s = ui.s;

        ui.rect(0.0, 0.0, wf, hf, [6, 10, 16, 140]);
        let blink = (self.anim * 2.0).fract() < 0.55;
        let slot = (58.0 * s).round();
        let pad = (20.0 * s).round();
        let head = (46.0 * s).round();
        let mw = 9.0 * slot + 2.0 * pad;
        let top_h = 4.0 * slot;
        let mh = head + top_h + 14.0 * s + 3.0 * slot + 10.0 * s + slot + pad;
        let bw = RECIPE_COLS as f32 * slot + 2.0 * pad + 8.0 * s;
        let gap = (10.0 * s).round();
        let bx = (cx - (bw + gap + mw) * 0.5).round();
        let mx = bx + bw + gap;
        let wy = ((hf - mh) * 0.5).round();
        let mut tip: Option<Stack> = None;
        let mut tip_text: Option<&str> = None;
        let stations = [(WORKBENCH, self.near_station(WORKBENCH)), (FURNACE, self.near_station(FURNACE))];
        let reachable = |b: Block| b == AIR || stations.iter().any(|&(st, near)| st == b && near);

        if self.mode == Mode::Creative {
            // ================= every item, free to take =================
            draw_window(&mut ui, bx, wy, bw, mh);
            ui.text(bx + pad, wy + 11.0 * s, HEAD, "All items", INK);
            let sh = (30.0 * s).round();
            let hot = draw_search(&mut ui, &font, bx + pad, wy + head, RECIPE_COLS as f32 * slot, sh, &self.search, self.search_on, blink);
            if ui.ptr.clicked {
                actions.push(Action::Search(hot));
            }
            let list: Vec<Item> = (1..items::ITEM_END).filter(|&i| items::valid(i) && search_hit(i, &self.search)).collect();
            let gy = wy + head + sh + 8.0 * s;
            if list.is_empty() {
                ui.text(bx + pad + 10.0 * s, gy + 12.0 * s, SMALL, "Nothing by that name.", INK_DIM);
            }
            let rows_fit = (((wy + mh - pad - gy) / slot).floor() as usize).max(1);
            let total_rows = (list.len() + RECIPE_COLS - 1) / RECIPE_COLS;
            let scroll = self.recipe_scroll.min(total_rows.saturating_sub(rows_fit));
            ui.rect(bx + pad - 2.0, gy - 2.0, RECIPE_COLS as f32 * slot + 4.0, rows_fit as f32 * slot + 4.0, INSET);
            for (k, &item) in list.iter().skip(scroll * RECIPE_COLS).take(RECIPE_COLS * rows_fit).enumerate() {
                let (x, y) = (bx + pad + (k % RECIPE_COLS) as f32 * slot, gy + (k / RECIPE_COLS) as f32 * slot);
                if draw_slot(&mut ui, x, y, slot, Stack::new(item, 1), false, true) {
                    tip = Some(Stack::new(item, 1));
                    if ui.ptr.clicked {
                        actions.push(Action::Take(item, self.shift));
                    }
                }
            }
            if total_rows > rows_fit {
                let track_h = rows_fit as f32 * slot;
                let bar_h = track_h * rows_fit as f32 / total_rows as f32;
                let bar_y = gy + (track_h - bar_h) * scroll as f32 / (total_rows - rows_fit) as f32;
                let sx = bx + pad + RECIPE_COLS as f32 * slot + 5.0 * s;
                ui.rect(sx, gy, 5.0 * s, track_h, INSET);
                ui.rect(sx, bar_y, 5.0 * s, bar_h, PAPER_DARK);
            }
        } else {
            // ================= recipe book =================
            draw_window(&mut ui, bx, wy, bw, mh);
            ui.text(bx + pad, wy + 11.0 * s, HEAD, "Recipes", INK);
            {
                // "craftable only" switch in the title row
                let label = "Can craft";
                let bsz = (18.0 * s).round();
                let lw = font.width(SMALL, label);
                let (tx, ty) = (bx + bw - pad - lw - bsz - 8.0 * s, wy + 16.0 * s);
                let hot = ui.hover(tx, ty - 3.0, lw + bsz + 8.0 * s, bsz + 6.0);
                ui.rect(tx, ty, bsz, bsz, [92, 82, 66, 255]);
                ui.rect(tx + 2.0, ty + 2.0, bsz - 4.0, bsz - 4.0, if self.recipe_craftable { INK_GOOD } else { INSET });
                ui.text(tx + bsz + 8.0 * s, ty - 1.0, SMALL, label, if hot { INK } else { INK_DIM });
                if hot && ui.ptr.clicked {
                    actions.push(Action::ToggleCraftable);
                }
            }
            let tab_w = ((bw - 2.0 * pad) / CATS.len() as f32).floor();
            let tab_h = (38.0 * s).round();
            let ty = wy + head;
            for (i, &(cat, label, icon)) in CATS.iter().enumerate() {
                let x = bx + pad + i as f32 * tab_w;
                let w = tab_w - 2.0;
                let on = cat == self.recipe_tab;
                let hot = ui.hover(x, ty, w, tab_h);
                ui.rect(
                    x,
                    ty,
                    w,
                    tab_h,
                    if on {
                        PAPER_LIGHT
                    } else if hot {
                        [200, 190, 170, 255]
                    } else {
                        INSET
                    },
                );
                ui.outline(x, ty, w, tab_h, 1.0, PAPER_DARK);
                if on {
                    ui.rect(x, ty + tab_h - (3.0 * s).round(), w, (3.0 * s).round(), INK);
                }
                let isz = (24.0 * s).round();
                ui.icon(x + (w - isz) * 0.5, ty + (tab_h - isz) * 0.5 - 1.0, isz, items::icon(icon), if on || hot { 255 } else { 190 });
                if hot {
                    tip_text = Some(label);
                    if ui.ptr.clicked {
                        actions.push(Action::Tab(cat));
                    }
                }
            }
            // the search box, under the tabs: a search looks through every tab
            let sh = (30.0 * s).round();
            let sy = ty + tab_h + 8.0 * s;
            let hot = draw_search(&mut ui, &font, bx + pad, sy, RECIPE_COLS as f32 * slot, sh, &self.search, self.search_on, blink);
            if ui.ptr.clicked {
                actions.push(Action::Search(hot));
            }
            let searching = !self.search.trim().is_empty();
            let list: Vec<usize> = (0..RECIPES.len())
                .filter(|&i| searching || self.recipe_tab == Cat::All || RECIPES[i].cat == self.recipe_tab)
                .filter(|&i| search_hit(RECIPES[i].out.0, &self.search))
                .filter(|&i| !self.recipe_craftable || (self.inv.can_craft(&RECIPES[i]) && reachable(RECIPES[i].station)))
                .collect();
            let gy = sy + sh + 8.0 * s;
            let rows_fit = (((wy + mh - pad - gy) / slot).floor() as usize).clamp(1, RECIPE_ROWS);
            let total_rows = (list.len() + RECIPE_COLS - 1) / RECIPE_COLS;
            let scroll = self.recipe_scroll.min(total_rows.saturating_sub(rows_fit));
            ui.rect(bx + pad - 2.0, gy - 2.0, RECIPE_COLS as f32 * slot + 4.0, rows_fit as f32 * slot + 4.0, INSET);
            if list.is_empty() {
                let why = if searching { "No recipe makes that." } else { "Nothing you can craft here yet." };
                ui.text(bx + pad + 10.0 * s, gy + 12.0 * s, SMALL, why, INK_DIM);
            }
            for (k, &ri) in list.iter().skip(scroll * RECIPE_COLS).take(RECIPE_COLS * rows_fit).enumerate() {
                let r = &RECIPES[ri];
                let (x, y) = (bx + pad + (k % RECIPE_COLS) as f32 * slot, gy + (k / RECIPE_COLS) as f32 * slot);
                let can = self.inv.can_craft(r) && reachable(r.station);
                let picked = ri == self.recipe_sel;
                let hot = ui.hover(x, y, slot, slot);
                let fill = if hot {
                    [226, 218, 200, 255]
                } else if can {
                    [202, 208, 170, 255]
                } else {
                    [150, 140, 124, 255]
                };
                ui.rect(x + 2.0, y + 2.0, slot - 4.0, slot - 4.0, fill);
                ui.outline(x + 2.0, y + 2.0, slot - 4.0, slot - 4.0, 1.0, if can { INK_GOOD } else { PAPER_DARK });
                if picked {
                    ui.outline(x, y, slot, slot, (3.0 * s).round().max(2.0), INK);
                }
                let ip = (slot * 0.18).round();
                ui.icon(x + ip, y + ip, slot - 2.0 * ip, items::icon(r.out.0), if can { 255 } else { 120 });
                if r.out.1 > 1 {
                    let n = r.out.1.to_string();
                    ui.label(x + slot - font.width(SMALL, &n) - 6.0 * s, y + slot - font.height(SMALL) - 2.0, SMALL, &n, TEXT);
                }
                if hot {
                    tip = Some(Stack::new(r.out.0, 1));
                    if ui.ptr.clicked {
                        actions.push(Action::PickRecipe(ri));
                    }
                }
            }
            if total_rows > rows_fit {
                let track_h = rows_fit as f32 * slot;
                let bar_h = track_h * rows_fit as f32 / total_rows as f32;
                let bar_y = gy + (track_h - bar_h) * scroll as f32 / (total_rows - rows_fit) as f32;
                let sx = bx + pad + RECIPE_COLS as f32 * slot + 5.0 * s;
                ui.rect(sx, gy, 5.0 * s, track_h, INSET);
                ui.rect(sx, bar_y, 5.0 * s, bar_h, PAPER_DARK);
            }
        }
        // ================= inventory window =================
        draw_window(&mut ui, mx, wy, mw, mh);
        ui.text(mx + pad, wy + 11.0 * s, HEAD, "Inventory", INK);
        {
            // vital signs and the sort button share the title row
            let sort_w = (64.0 * s).round();
            let sort_h = (26.0 * s).round();
            let (sx, sy) = (mx + mw - pad - sort_w, wy + 12.0 * s);
            let hot = ui.hover(sx, sy, sort_w, sort_h);
            ui.rect(sx, sy, sort_w, sort_h, if hot { PAPER_LIGHT } else { INSET });
            ui.outline(sx, sy, sort_w, sort_h, 1.0, PAPER_DARK);
            ui.text_centered(sx + sort_w * 0.5, sy + (sort_h - font.height(SMALL)) * 0.5, SMALL, "Sort", INK);
            if hot && ui.ptr.clicked {
                actions.push(Action::Sort);
            }
            let isz = font.height(SMALL).round();
            let mut x = sx - 14.0 * s;
            let armour = format!("Armour {:.0}%", (1.0 - self.inv.armor_factor()) * 100.0);
            for (icon, text) in
                [(0, armour), (items::ICON_FOOD, format!("{:.0}", self.food.ceil())), (items::ICON_HEART, format!("{:.0}", self.health.ceil()))]
            {
                x -= font.width(SMALL, &text);
                ui.text(x, sy + (sort_h - isz) * 0.5, SMALL, &text, INK);
                if icon != 0 {
                    x -= isz + 4.0 * s;
                    ui.icon(x, sy + (sort_h - isz) * 0.5, isz, icon, 255);
                }
                x -= 14.0 * s;
            }
        }
        let mut clicks: Vec<Action> = Vec::new();
        let slot_at = |ui: &mut Ui, tip: &mut Option<Stack>, clicks: &mut Vec<Action>, idx: usize, x: f32, y: f32, selected: bool| {
            let st = self.inv.slots[idx];
            let hot = draw_slot(ui, x, y, slot, st, selected, true);
            if !st.is_empty() && !self.search.trim().is_empty() {
                if search_hit(st.item, &self.search) {
                    // what the search is after stands out
                    ui.outline(x + 1.0, y + 1.0, slot - 2.0, slot - 2.0, (3.0 * ui.s).round().max(2.0), INK_GOOD);
                } else {
                    ui.rect(x + 2.0, y + 2.0, slot - 4.0, slot - 4.0, [40, 34, 28, 170]);
                }
            }
            if hot {
                if self.drag.is_some() {
                    clicks.push(Action::DragVisit(controls::Slot::Player(idx)));
                }
                if !st.is_empty() {
                    *tip = Some(st);
                }
                if ui.ptr.clicked {
                    clicks.push(Action::Slot(idx, false));
                } else if ui.ptr.rclicked {
                    clicks.push(Action::Slot(idx, true));
                }
            }
        };
        // armour column: an empty slot shows the outline of what goes in it
        let ty = wy + head;
        let ghosts = [items::IRON_HELMET, items::IRON_CHEST, items::IRON_LEGS, items::IRON_BOOTS];
        for i in 0..4 {
            let (x, y) = (mx + pad, ty + i as f32 * slot);
            let empty = self.inv.slots[items::ARMOR + i].is_empty();
            slot_at(&mut ui, &mut tip, &mut clicks, items::ARMOR + i, x, y, false);
            if empty {
                let ip = (slot * 0.2).round();
                ui.icon(x + ip, y + ip, slot - 2.0 * ip, items::icon(ghosts[i]), 70);
            }
        }
        // the player, dressed in whatever is worn
        let fx = mx + pad + slot + 8.0 * s;
        let fw = (150.0 * s).round();
        ui.rect(fx, ty + 2.0, fw, top_h - 4.0, [58, 52, 46, 255]);
        ui.outline(fx, ty + 2.0, fw, top_h - 4.0, 2.0, [92, 82, 66, 255]);
        {
            let u = ((top_h - 4.0) / 19.0).floor();
            let (ox, oy) = ((fx + fw * 0.5).round(), ty + 2.0 + ((top_h - 4.0) - 16.0 * u) * 0.5);
            let worn = |i: usize| -> Option<[u8; 4]> {
                items::gear(self.inv.slots[items::ARMOR + i].item).map(|(_, m)| match m {
                    items::Mat::Leather => [150, 98, 58, 255],
                    items::Mat::Iron => [216, 216, 222, 255],
                    items::Mat::Gold => [250, 208, 66, 255],
                    items::Mat::Pyre => [196, 30, 52, 255],
                    _ => [104, 222, 232, 255],
                })
            };
            let dark = |c: [u8; 4]| [(c[0] as f32 * 0.72) as u8, (c[1] as f32 * 0.72) as u8, (c[2] as f32 * 0.72) as u8, 255];
            let (skin, shirt, trousers, shoes) = ([226, 176, 134, 255], [176, 92, 52, 255], [104, 94, 76, 255], [52, 44, 38, 255]);
            // head
            ui.rect(ox - 2.0 * u, oy, 4.0 * u, 4.0 * u, skin);
            ui.rect(ox - 2.0 * u, oy, 4.0 * u, u, [38, 32, 30, 255]);
            ui.rect(ox - 1.2 * u, oy + 1.8 * u, 0.7 * u, 0.7 * u, [40, 40, 60, 255]);
            ui.rect(ox + 0.5 * u, oy + 1.8 * u, 0.7 * u, 0.7 * u, [40, 40, 60, 255]);
            if let Some(c) = worn(0) {
                ui.rect(ox - 2.2 * u, oy - 0.2 * u, 4.4 * u, 1.6 * u, c);
                ui.rect(ox - 2.2 * u, oy + 1.4 * u, 0.7 * u, 2.2 * u, dark(c));
                ui.rect(ox + 1.5 * u, oy + 1.4 * u, 0.7 * u, 2.2 * u, dark(c));
            }
            // body and arms
            let by = oy + 4.0 * u;
            let top_c = worn(1).unwrap_or(shirt);
            ui.rect(ox - 2.0 * u, by, 4.0 * u, 6.0 * u, top_c);
            ui.rect(ox - 4.0 * u, by, 2.0 * u, 6.0 * u, dark(top_c));
            ui.rect(ox + 2.0 * u, by, 2.0 * u, 6.0 * u, dark(top_c));
            if worn(1).is_none() {
                ui.rect(ox - 4.0 * u, by + 2.0 * u, 2.0 * u, 4.0 * u, skin);
                ui.rect(ox + 2.0 * u, by + 2.0 * u, 2.0 * u, 4.0 * u, skin);
            } else {
                ui.rect(ox - 4.0 * u, by + 5.0 * u, 2.0 * u, u, skin);
                ui.rect(ox + 2.0 * u, by + 5.0 * u, 2.0 * u, u, skin);
            }
            // legs and feet
            let ly = by + 6.0 * u;
            let leg_c = worn(2).unwrap_or(trousers);
            ui.rect(ox - 2.0 * u, ly, 2.0 * u - 1.0, 6.0 * u, leg_c);
            ui.rect(ox + 1.0, ly, 2.0 * u - 1.0, 6.0 * u, dark(leg_c));
            let foot_c = worn(3).unwrap_or(shoes);
            let fh = if worn(3).is_some() { 2.0 * u } else { u };
            ui.rect(ox - 2.0 * u, ly + 6.0 * u - fh, 2.0 * u - 1.0, fh, foot_c);
            ui.rect(ox + 1.0, ly + 6.0 * u - fh, 2.0 * u - 1.0, fh, dark(foot_c));
        }
        // crafting: the picked recipe's ingredients, an arrow, and the result to take
        let kx = fx + fw + 14.0 * s;
        let kw = mx + mw - pad - kx;
        let stations_available = self.near_station(ANVIL) || self.near_station(ENCHANTING_TABLE);
        let button_w = if stations_available { (kw - 5.0 * s) * 0.5 } else { kw };
        if ui.button(kx, ty + 2.0 * s, button_w, 25.0 * s, if self.grid_open { "Recipes" } else { "Grid" }, true) {
            clicks.push(Action::ToggleGrid);
        }
        if stations_available
            && ui.button(kx + button_w + 5.0 * s, ty + 2.0 * s, button_w, 25.0 * s, if self.equipment_open { "Recipes" } else { "Equipment" }, true)
        {
            clicks.push(Action::ToggleEquipment);
        }
        {
            let ty = ty + 32.0 * s;
            if self.grid_open {
                let bench = self.near_station(WORKBENCH);
                let size = if bench { 3 } else { 2 };
                let small = 45.0 * s;
                ui.text(kx, ty + 3.0 * s, SMALL, if bench { "Workbench: 3 x 3" } else { "Inventory: 2 x 2" }, INK_DIM);
                for y in 0..size {
                    for x in 0..size {
                        let i = y * 3 + x;
                        let hot = draw_slot(&mut ui, kx + x as f32 * small, ty + 27.0 * s + y as f32 * small, small, self.inv.grid[i], false, true);
                        if hot {
                            tip = Some(self.inv.grid[i]);
                            if self.drag.is_some() {
                                clicks.push(Action::DragVisit(controls::Slot::Grid(i)));
                            }
                            if ui.ptr.clicked || ui.ptr.rclicked {
                                clicks.push(Action::GridSlot(i, ui.ptr.rclicked));
                            }
                        }
                    }
                }
                let out =
                    crafting::recipe(&self.inv.grid, size, bench).map(|i| equipment::craft_output(&RECIPES[i], &self.inv.grid)).unwrap_or_default();
                if draw_slot(&mut ui, kx + 3.0 * small + 18.0 * s, ty + 60.0 * s, small, out, false, true) && ui.ptr.clicked && !out.is_empty() {
                    clicks.push(Action::GridCraft(self.shift));
                }
                ui.text(kx, ty + 166.0 * s, SMALL, "Left drag divides stacks", INK_DIM);
                ui.text(kx, ty + 183.0 * s, SMALL, "Right drag places one", INK_DIM);
            } else if self.equipment_open && stations_available {
                ui.text(kx, ty + 6.0 * s, BODY, "Equipment", INK);
                ui.text(kx, ty + 32.0 * s, SMALL, "Pick equipment onto the cursor", INK_DIM);
                ui.text(kx, ty + 52.0 * s, SMALL, &format!("{} levels | 1 crystal per enchant", self.level), INK_DIM);
                if self.near_station(ANVIL) && ui.button(kx, ty + 75.0 * s, kw, 26.0 * s, "Repair (1 level + material)", self.inv.held.wear > 0) {
                    clicks.push(Action::Repair);
                }
                if self.near_station(ENCHANTING_TABLE) {
                    for (j, kind) in equipment::Enchant::ALL.into_iter().enumerate() {
                        let label =
                            format!("{} {} ({} levels)", kind.name(), kind.level(self.inv.held) + 1, (kind.level(self.inv.held) as u32 + 1) * 3);
                        if ui.button(
                            kx,
                            ty + (105.0 + j as f32 * 29.0) * s,
                            kw,
                            27.0 * s,
                            &label,
                            kind.accepts(self.inv.held.item) && kind.level(self.inv.held) < 3,
                        ) {
                            clicks.push(Action::Enchant(kind));
                        }
                    }
                }
            } else {
                let r = &RECIPES[self.recipe_sel.min(RECIPES.len() - 1)];
                let at_station = reachable(r.station);
                let can = self.inv.can_craft(r) && at_station;
                ui.text(kx, ty + 2.0 * s, SMALL, "Crafting", INK_DIM);
                let ing = (50.0 * s).round();
                let iy = ty + 26.0 * s;
                for k in 0..4 {
                    let (x, y) = (kx + (k % 2) as f32 * ing, iy + (k / 2) as f32 * ing);
                    match r.ins.get(k) {
                        Some(&(item, need)) => {
                            let have = self.inv.count(item);
                            let hot = draw_slot(&mut ui, x, y, ing, Stack::new(item, need), false, true);
                            ui.outline(x + 1.0, y + 1.0, ing - 2.0, ing - 2.0, 2.0, if have >= need { INK_GOOD } else { INK_BAD });
                            if hot {
                                tip = Some(Stack::new(item, 1));
                            }
                        }
                        None => {
                            draw_slot(&mut ui, x, y, ing, Stack::default(), false, true);
                        }
                    }
                }
                // arrow
                let (ax, ay) = (kx + 2.0 * ing + 10.0 * s, iy + ing);
                let ac = if can { INK } else { [150, 140, 124, 255] };
                ui.rect(ax, ay - 3.0 * s, 20.0 * s, 6.0 * s, ac);
                for k in 0..5 {
                    let h = (5 - k) as f32 * 4.0 * s;
                    ui.rect(ax + 20.0 * s + k as f32 * 3.0 * s, ay - h * 0.5, 3.0 * s, h, ac);
                }
                // result
                let big = (66.0 * s).round();
                let (ox, oy) = ((kx + kw - big).min(ax + 46.0 * s), iy + ing - big * 0.5);
                let out = Stack::new(r.out.0, r.out.1);
                let hot = ui.hover(ox, oy, big, big);
                draw_slot(&mut ui, ox, oy, big, Stack::default(), false, true);
                if can {
                    ui.outline(ox, oy, big, big, (3.0 * s).round().max(2.0), INK_GOOD);
                    draw_stack(&mut ui, ox, oy, big, out);
                } else {
                    let ip = (big * 0.16).round();
                    ui.icon(ox + ip, oy + ip, big - 2.0 * ip, items::icon(r.out.0), 110);
                }
                if hot {
                    tip = Some(out);
                    if can && ui.ptr.clicked {
                        clicks.push(Action::CraftHand(self.shift));
                    }
                }
                let mut ly = iy + 2.0 * ing + 8.0 * s;
                ui.text(kx, ly, BODY, items::name(r.out.0), INK);
                ly += font.height(BODY) + 1.0 * s;
                let missing: Vec<String> = r
                    .ins
                    .iter()
                    .filter(|&&(i, n)| self.inv.count(i) < n)
                    .map(|&(i, n)| format!("{} {}", n - self.inv.count(i), items::name(i)))
                    .collect();
                let (line, col) = if !at_station {
                    (format!("Stand near a {}", items::name(r.station)), INK_BAD)
                } else if !missing.is_empty() {
                    (format!("Need {}", missing.join(", ")), INK_BAD)
                } else if r.station != AIR {
                    (format!("Ready at the {}", items::name(r.station)), INK_GOOD)
                } else {
                    ("Ready to craft by hand".to_string(), INK_GOOD)
                };
                // long lines shrink to what fits
                let mut line = line;
                while font.width(SMALL, &line) > kw && line.len() > 4 {
                    line.truncate(line.len() - 4);
                    line.push_str("...");
                }
                ui.text(kx, ly, SMALL, &line, col);
            }
        }
        // backpack and hotbar
        let gy = ty + top_h + 14.0 * s;
        for i in items::HOTBAR..items::BAG_END {
            let k = i - items::HOTBAR;
            slot_at(&mut ui, &mut tip, &mut clicks, i, mx + pad + (k % 9) as f32 * slot, gy + (k / 9) as f32 * slot, false);
        }
        let hy2 = gy + 3.0 * slot + 10.0 * s;
        for i in 0..items::HOTBAR {
            slot_at(&mut ui, &mut tip, &mut clicks, i, mx + pad + i as f32 * slot, hy2, i == self.sel);
        }
        actions.append(&mut clicks);
        if self.toast.1 > 0.0 {
            ui.label_centered(cx, wy + mh + 10.0 * s, BODY, &self.toast.0, ACCENT);
        }

        // the stack on the mouse, or a tooltip for what is under it
        let held = self.inv.held;
        let (px, py) = (ui.ptr.x, ui.ptr.y);
        if !held.is_empty() {
            draw_stack(&mut ui, px - slot * 0.5, py - slot * 0.5, slot, held);
            // clicking outside the windows throws it on the ground
            if ui.ptr.clicked && !ui.hover(bx, wy, bw + gap + mw, mh) {
                actions.push(Action::DropHeld);
            }
        } else if let Some(st) = tip {
            draw_tip(&mut ui, st, wf, hf);
        } else if let Some(text) = tip_text {
            let tw = font.width(SMALL, text) + 20.0 * s;
            let th = font.height(SMALL) + 12.0 * s;
            ui.rect(px + 14.0 * s, py + 18.0 * s, tw, th, [16, 14, 22, 246]);
            ui.outline(px + 14.0 * s, py + 18.0 * s, tw, th, 1.0, ACCENT);
            ui.text(px + 24.0 * s, py + 24.0 * s, SMALL, text, TEXT);
        }
    }
}

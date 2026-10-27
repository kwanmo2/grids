use crate::*;

// ---------- tabs: one Explorer view per pane, the tabs are folders it switches between ----------

// View menu: show / hide every pane's strip. The tabs stay put while hidden.
pub(crate) unsafe fn toggle_tabs() {
    if let Some(p) = app(|a| a.tab_press) {
        cancel_tab_drag(p.hwnd);
    }
    let on = app(|a| {
        a.s.tabs_on ^= true;
        a.s.tabs_on
    });
    TABS_ON.store(on, Ordering::Relaxed);
    for h in app(|a| a.panes.iter().flatten().map(|p| p.hwnd).collect::<Vec<_>>()) {
        layout_pane(h);
        _ = InvalidateRect(Some(h), None, false);
    }
}

pub(crate) unsafe fn repaint_tabs(i: usize) {
    let Some(h) = app(|a| a.panes.get(i)?.as_ref().map(|p| p.hwnd)) else { return };
    let mut rc = RECT::default();
    _ = GetClientRect(h, &mut rc);
    let strip = pane_geo(rc.right, rc.bottom, app(|a| a.dpi)).tabs;
    if strip.bottom > strip.top {
        _ = InvalidateRect(Some(h), Some(&strip), false);
    }
}

// A tab change that browses: undone when the folder can't be opened (go_near already tried its parent).
unsafe fn tab_go(i: usize, change: impl FnOnce(&mut Session) -> Option<String>) {
    let (before, to) = app(|a| {
        let before = (a.s.tabs[i].clone(), a.s.tab[i], a.s.tab_colors[i].clone());
        (before, change(&mut a.s))
    });
    if let Some(to) = to {
        if !go_near(i, &to) {
            app(|a| (a.s.tabs[i], a.s.tab[i], a.s.tab_colors[i]) = before);
            _ = MessageBeep(MB_ICONWARNING);
        }
    }
    repaint_tabs(i);
}

pub(crate) unsafe fn new_tab(i: usize) {
    if !tabs_on() {
        toggle_tabs();
    }
    app(|a| a.s.add_tab(i));
    repaint_tabs(i);
}

pub(crate) unsafe fn pick_tab(i: usize, k: usize) {
    tab_go(i, |s| s.pick_tab(i, k));
}

pub(crate) unsafe fn dup_tab(i: usize, k: usize) {
    tab_go(i, |s| s.dup_tab(i, k));
}

// Right-click on a tab: duplicate or close that tab.
pub(crate) unsafe fn tab_menu(h: HWND, i: usize, k: usize, at: POINT) {
    let n = app(|a| a.s.tab_list(i).len());
    let Ok(menu) = CreatePopupMenu() else { return };
    _ = AppendMenuW(menu, MF_STRING, 1, &HSTRING::from(tr("Duplicate tab\tCtrl+K", "탭 복제\tCtrl+K")));
    let close = if n > 1 { MF_STRING } else { MF_STRING | MF_GRAYED }; // the last tab stays
    _ = AppendMenuW(menu, close, 2, &HSTRING::from(tr("Close tab\tCtrl+W", "탭 닫기\tCtrl+W")));
    set_active(i);
    let pick = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN, at.x, at.y, None, h, None).0;
    _ = DestroyMenu(menu);
    match pick {
        1 => dup_tab(i, k),
        2 => close_tab(i, k),
        _ => {}
    }
    focus_view(i);
}

pub(crate) unsafe fn close_tab(i: usize, k: usize) {
    if app(|a| a.s.tab_list(i).len()) < 2 {
        _ = MessageBeep(MB_ICONWARNING); // the last tab stays
        return;
    }
    tab_go(i, |s| s.close_tab(i, k));
}

// Ctrl+PgDn / Ctrl+PgUp, wrapping around.
pub(crate) unsafe fn step_tab(i: usize, d: i32) {
    let (n, cur) = app(|a| (a.s.tab_list(i).len() as i32, a.s.tab[i] as i32));
    if n > 1 {
        pick_tab(i, (cur + d).rem_euclid(n) as usize);
    }
}

// Every tab's full path, including those outside the visible strip. Escape menu mnemonic characters.
pub(crate) unsafe fn all_tabs_menu(h: HWND, i: usize, at: RECT) {
    let (tabs, cur) = app(|a| (a.s.tab_list(i), a.s.tab[i]));
    let Ok(menu) = CreatePopupMenu() else { return };
    for (k, path) in tabs.iter().enumerate() {
        let label = format!("{}  {}", k + 1, path.replace('&', "&&").replace('\t', " "));
        let flags = if k == cur { MF_STRING | MF_CHECKED } else { MF_STRING };
        _ = AppendMenuW(menu, flags, k + 1, &HSTRING::from(label));
    }
    let mut p = POINT { x: at.left, y: at.bottom };
    _ = ClientToScreen(h, &mut p);
    let pick = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_LEFTALIGN | TPM_TOPALIGN, p.x, p.y, None, h, None).0;
    _ = DestroyMenu(menu);
    if pick > 0 {
        pick_tab(i, pick as usize - 1);
    }
}

// A header-window mouse message landing in the tab strip.
pub(crate) unsafe fn on_strip(h: HWND, _i: usize, lp: LPARAM) -> bool {
    let (x, y) = ((lp.0 & 0xffff) as i16 as i32, (lp.0 >> 16 & 0xffff) as i16 as i32);
    let mut rc = RECT::default();
    _ = GetClientRect(h, &mut rc);
    inside(pane_geo(rc.right, rc.bottom, app(|a| a.dpi)).tabs, x, y)
}

#[derive(Debug, PartialEq)]
pub(crate) enum TabHit {
    Tab(usize),
    Plus,
    More,
    Strip,
}

// What is under (x, y) in a pane's strip; None outside it (or with tabs off).
pub(crate) fn tab_hit(pg: &PaneGeo, n: usize, cur: usize, x: i32, y: i32, dpi: i32) -> Option<TabHit> {
    if !inside(pg.tabs, x, y) {
        return None;
    }
    let geo = tab_rects(pg.tabs, n, cur, dpi);
    if inside(geo.plus, x, y) {
        return Some(TabHit::Plus);
    }
    if geo.more.is_some_and(|r| inside(r, x, y)) {
        return Some(TabHit::More);
    }
    let Some(visible) = geo.tabs.iter().position(|r| inside(*r, x, y)) else { return Some(TabHit::Strip) };
    Some(TabHit::Tab(geo.first + visible))
}

// Mouse in a pane's header window: defer tab selection until release so a drag never browses.
pub(crate) unsafe fn tab_mouse(h: HWND, i: usize, x: i32, y: i32, middle: bool) -> bool {
    let mut rc = RECT::default();
    _ = GetClientRect(h, &mut rc);
    let dpi = app(|a| a.dpi);
    let (n, cur) = app(|a| (a.s.tab_list(i).len(), a.s.tab[i]));
    let pg = pane_geo(rc.right, rc.bottom, dpi);
    let Some(hit) = tab_hit(&pg, n, cur, x, y, dpi) else { return false };
    set_active(i);
    match (hit, middle) {
        (TabHit::Tab(k), true) => {
            cancel_tab_drag(h);
            close_tab(i, k);
        }
        (TabHit::Tab(k), false) => {
            let first = tab_rects(pg.tabs, n, cur, dpi).first;
            app(|a| a.tab_press = Some(TabPress { hwnd: h, from: k, x0: x, y0: y, x, y, live: false, slot: None, first }));
            SetCapture(h);
            SetTimer(Some(h), TIMER_TAB_DRAG, 160, None);
            return true;
        }
        (TabHit::Plus, false) => new_tab(i),
        (TabHit::More, false) => {
            if let Some(r) = tab_rects(pg.tabs, n, cur, dpi).more {
                all_tabs_menu(h, i, r);
            }
        }
        _ => {}
    }
    focus_view(i);
    true
}

// Each tab has its own colour band. The active one has a tinted plate and a thicker band.
pub(crate) unsafe fn paint_tabs(dc: HDC, pg: &PaneGeo, tabs: &[String], colors: &[u8], cur: usize, dpi: i32, font: HFONT, press: Option<TabPress>) {
    let s = |v: i32| v * dpi / 96;
    if pg.tabs.bottom <= pg.tabs.top {
        return;
    }
    fill(dc, pg.tabs, pal().bg);
    let geo = tab_drag_geo(pg.tabs, tabs.len(), cur, dpi, press);
    for (offset, r) in geo.tabs.iter().enumerate() {
        let k = geo.first + offset;
        let path = &tabs[k];
        let on = k == cur;
        let color = pal().accents[colors[k] as usize].1;
        let dragging = press.is_some_and(|p| p.live && p.from == k);
        let name = match short_name(path) {
            n if n.is_empty() => "…".to_string(),
            n => n,
        };
        if r.right <= r.left {
            continue;
        }
        if on || dragging {
            fill(dc, *r, blend(pal().card, color, if dragging { 0.25 } else { 0.12 }));
        }
        let band = tab_band(*r, dpi, on);
        fill(dc, band, color);
        fill(dc, RECT { left: r.right - 1, top: band.bottom, bottom: r.bottom, ..*r }, pal().line);
        if on {
            fill(dc, RECT { top: r.bottom - s(2), bottom: r.bottom, ..band }, color);
        }
        let label = RECT { left: r.left + s(8), right: (r.right - s(8)).max(r.left + s(8)), top: band.bottom, bottom: r.bottom };
        text(dc, label, &name, font, if on { pal().text } else { pal().sub }, DT_LEFT);
    }
    text(dc, geo.plus, "+", font, pal().sub, DT_CENTER);
    if let Some(r) = geo.more {
        text(dc, r, "▾", font, pal().sub, DT_CENTER);
    }
    if let Some(r) = tab_drag_marker(&geo, pg.tabs, press, dpi) {
        fill(dc, r, pal().text);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct TabPress {
    pub(crate) hwnd: HWND,
    pub(crate) from: usize,
    pub(crate) x0: i32,
    pub(crate) y0: i32,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) live: bool,
    pub(crate) slot: Option<usize>,
    pub(crate) first: usize,
}

impl TabPress {
    pub(crate) fn update(&mut self, x: i32, y: i32, threshold: (i32, i32)) {
        self.x = x;
        self.y = y;
        self.live |= (x - self.x0).abs() > threshold.0 || (y - self.y0).abs() > threshold.1;
    }
}

pub(crate) fn tab_band(tab: RECT, dpi: i32, active: bool) -> RECT {
    let gap = (2 * dpi / 96).min((tab.right - tab.left).max(0) / 2);
    RECT { left: tab.left + gap, right: tab.right - gap, bottom: (tab.top + (if active { 4 } else { 3 }) * dpi / 96).min(tab.bottom), ..tab }
}

pub(crate) fn tab_drag_geo(strip: RECT, n: usize, cur: usize, dpi: i32, press: Option<TabPress>) -> TabGeo {
    let mut geo = tab_rects(strip, n, cur, dpi);
    if let Some(p) = press.filter(|p| p.live) {
        geo.first = p.first.min(n - geo.tabs.len());
    }
    geo
}

pub(crate) fn tab_drop_slot(geo: &TabGeo, strip: RECT, x: i32, y: i32) -> Option<usize> {
    (inside(strip, x, y) && !geo.tabs.is_empty()).then(|| geo.first + geo.tabs.iter().filter(|r| x >= r.left + (r.right - r.left) / 2).count())
}

pub(crate) fn tab_drag_marker(geo: &TabGeo, strip: RECT, press: Option<TabPress>, dpi: i32) -> Option<RECT> {
    let p = press.filter(|p| p.live)?;
    let slot = p.slot.filter(|&k| k != p.from && k != p.from + 1 && k >= geo.first && k <= geo.first + geo.tabs.len())?;
    let x = if slot < geo.first + geo.tabs.len() { geo.tabs[slot - geo.first].left } else { geo.tabs.last()?.right };
    let width = (2 * dpi / 96).max(1);
    Some(RECT { left: (x - width / 2).max(strip.left), right: (x + width).min(strip.right), top: strip.top + 2 * dpi / 96, bottom: strip.bottom })
}

pub(crate) fn tab_scroll_dir(geo: &TabGeo, strip: RECT, n: usize, x: i32, y: i32, dpi: i32) -> i32 {
    if !inside(strip, x, y) {
        return 0;
    }
    match (geo.tabs.first(), geo.tabs.last()) {
        (Some(first), _) if geo.first > 0 && x < first.left + 16 * dpi / 96 => -1,
        (_, Some(last)) if geo.first + geo.tabs.len() < n && x >= last.right - 16 * dpi / 96 => 1,
        _ => 0,
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum TabRelease {
    None,
    Pick(usize),
    Move(usize, usize),
}

pub(crate) fn tab_release(p: TabPress, geo: &TabGeo, strip: RECT, x: i32, y: i32) -> TabRelease {
    if p.live {
        return tab_drop_slot(geo, strip, x, y).map_or(TabRelease::None, |slot| TabRelease::Move(p.from, slot));
    }
    match geo.tabs.iter().position(|r| inside(*r, x, y)) {
        Some(k) if geo.first + k == p.from => TabRelease::Pick(p.from),
        _ => TabRelease::None,
    }
}

unsafe fn drag_geo(h: HWND, press: Option<TabPress>) -> Option<(usize, PaneGeo, TabGeo, i32, usize)> {
    let i = pane_of(h)?;
    let (n, cur, dpi) = app(|a| (a.s.tab_list(i).len(), a.s.tab[i], a.dpi));
    let mut rc = RECT::default();
    _ = GetClientRect(h, &mut rc);
    let pg = pane_geo(rc.right, rc.bottom, dpi);
    let geo = tab_drag_geo(pg.tabs, n, cur, dpi, press);
    Some((i, pg, geo, dpi, n))
}

pub(crate) unsafe fn cancel_tab_drag(h: HWND) {
    let p = app(|a| if a.tab_press.is_some_and(|p| p.hwnd == h) { a.tab_press.take() } else { None });
    if p.is_some() {
        _ = KillTimer(Some(h), TIMER_TAB_DRAG);
        if GetCapture() == h {
            _ = ReleaseCapture();
        }
        if let Some(i) = pane_of(h) {
            repaint_tabs(i);
        }
    }
}

pub(crate) unsafe fn move_tab_drag(h: HWND, x: i32, y: i32) -> bool {
    let Some(mut p) = app(|a| a.tab_press.filter(|p| p.hwnd == h)) else { return false };
    let before = p;
    p.update(x, y, (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)));
    if let Some((i, pg, geo, _, _)) = drag_geo(h, Some(p)) {
        if p.live {
            p.slot = tab_drop_slot(&geo, pg.tabs, x, y);
        }
        app(|a| a.tab_press = Some(p));
        if p != before {
            repaint_tabs(i);
        }
    }
    true
}

pub(crate) unsafe fn scroll_tab_drag(h: HWND) {
    let Some(mut p) = app(|a| a.tab_press.filter(|p| p.hwnd == h && p.live)) else { return };
    let Some((i, pg, mut geo, dpi, n)) = drag_geo(h, Some(p)) else { return };
    let dir = tab_scroll_dir(&geo, pg.tabs, n, p.x, p.y, dpi);
    if dir != 0 {
        p.first = (geo.first as i32 + dir).clamp(0, (n - geo.tabs.len()) as i32) as usize;
        geo.first = p.first;
        p.slot = tab_drop_slot(&geo, pg.tabs, p.x, p.y);
        app(|a| a.tab_press = Some(p));
        repaint_tabs(i);
    }
}

pub(crate) unsafe fn finish_tab_drag(h: HWND, x: i32, y: i32) -> bool {
    let Some(p) = app(|a| a.tab_press.filter(|p| p.hwnd == h)) else { return false };
    let target = drag_geo(h, Some(p));
    cancel_tab_drag(h);
    if let Some((i, pg, geo, _, _)) = target {
        match tab_release(p, &geo, pg.tabs, x, y) {
            TabRelease::Pick(k) => pick_tab(i, k),
            TabRelease::Move(from, slot) => {
                app(|a| a.s.move_tab(i, from, slot));
                repaint_tabs(i);
            }
            TabRelease::None => {}
        }
        focus_view(i);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pressed(from: usize, x: i32, y: i32) -> TabPress {
        TabPress { hwnd: HWND::default(), from, x0: x, y0: y, x, y, live: false, slot: None, first: 0 }
    }

    #[test]
    fn reorder_preserves_active_folder_and_colors() {
        for cur in 0..4 {
            for from in 0..4 {
                for slot in 0..=4 {
                    let mut s = Session::default();
                    s.tabs[0] = ["a", "b", "c", "d"].map(String::from).to_vec();
                    s.tab[0] = cur;
                    s.paths[0] = "current folder".into(); // live folder wins over the stale stored entry
                    s.tab_colors[0] = vec![0, 2, 3, 4];
                    s.paths[1] = "other pane".into();
                    s.add_tab(1);
                    let before = s.clone();
                    let paths = s.tab_list(0);
                    let mut order: Vec<usize> = (0..4).collect();
                    let moved = s.move_tab(0, from, slot);
                    assert_eq!(moved, slot != from && slot != from + 1);
                    if moved {
                        let id = order.remove(from);
                        order.insert(if slot > from { slot - 1 } else { slot }, id);
                    }
                    assert_eq!(s.tab_list(0), order.iter().map(|&k| paths[k].clone()).collect::<Vec<_>>());
                    assert_eq!(s.tab_color_list(0), order.iter().map(|&k| before.tab_colors[0][k]).collect::<Vec<_>>());
                    assert_eq!(s.tab[0], order.iter().position(|&k| k == cur).unwrap());
                    assert_eq!(s.paths, before.paths);
                    assert_eq!(s.tab_list(1), before.tab_list(1));
                    assert_eq!(s.tab_colors[1], before.tab_colors[1]);
                    let before = s.clone();
                    assert!(!s.move_tab(0, 4, 0) && !s.move_tab(0, 0, 5));
                    assert_eq!(s, before);
                }
            }
        }
    }

    #[test]
    fn reordered_tabs_survive_saved_session_and_restart() {
        let mut s = Session::default();
        for i in 0..MAXP {
            s.paths[i] = format!("C:\\프로젝트 {i}\\원본");
            s.add_tab(i);
            s.paths[i] = format!("C:\\프로젝트 {i}\\출력");
            s.add_tab(i);
            s.paths[i] = format!("C:\\프로젝트 {i}\\백업");
            s.tab_colors[i].rotate_left(i % 3);
            assert!(s.move_tab(i, 2, 0));
        }
        s.active = 2;
        let file = std::env::temp_dir().join(format!("grids-tabs-{}.grids", std::process::id()));
        std::fs::write(&file, s.snapshot()).unwrap();
        let saved = Session::parse(&std::fs::read_to_string(&file).unwrap());
        std::fs::remove_file(&file).unwrap();
        let mut loaded = Session::default();
        loaded.take_layout(&saved);
        let restarted = Session::parse(&s.to_ini());
        for restored in [loaded, restarted] {
            assert_eq!(restored.active, s.active);
            for i in 0..MAXP {
                assert_eq!(restored.tab_list(i), s.tab_list(i));
                assert_eq!(restored.tab[i], s.tab[i]);
                assert_eq!(restored.tab_color_list(i), s.tab_color_list(i));
            }
        }
        let mut swapped = s.clone();
        swapped.swap_slots(0, 1);
        assert_eq!(swapped.tab_color_list(0), s.tab_color_list(1));
        let mut order: Vec<u8> = (0..MAXP as u8).collect();
        order.swap(0, 1);
        swapped.reorder(&order);
        assert_eq!(swapped.tab_colors, s.tab_colors);
    }

    #[test]
    fn colors_follow_tab_creation_and_closing() {
        let mut s = Session::default();
        s.paths[0] = "a".into();
        for _ in 0..12 {
            s.add_tab(0);
        }
        assert!(s.tab_color_list(0).windows(2).all(|w| w[0] != w[1]));
        let keep = s.tab_color_list(0);
        s.dup_tab(0, 3);
        let colors = s.tab_color_list(0);
        assert_ne!(colors[3], colors[4]);
        assert_ne!(colors[4], colors[5]);
        assert_eq!(colors[..4], keep[..4]);
        assert_eq!(colors[5..], keep[4..]);
        s.close_tab(0, 4);
        assert_eq!(s.tab_color_list(0), keep);
        while s.tab_list(0).len() > 1 {
            s.close_tab(0, 0);
        }
        let color = s.tab_color_list(0);
        assert_eq!(Session::parse(&s.to_ini()).tab_color_list(0), color);
        let old = Session::parse("p0=a\nb0=0|a|b|c\nl0=4,no,99\n");
        assert_eq!(old.tab_color_list(0), vec![4, TAB_COLORS[1], TAB_COLORS[2]]);
        assert_eq!(Session::parse("p0=a\nb0=0|a|b|c\n").tab_color_list(0), TAB_COLORS[..3]);
    }

    #[test]
    fn click_drag_and_cancel_are_distinct() {
        let strip = RECT { left: 2, top: 30, right: 602, bottom: 56 };
        let geo = tab_rects(strip, 3, 0, 96);
        let (x, y) = (geo.tabs[0].left + 20, geo.tabs[0].top + 10);
        let mut p = pressed(0, x, y);
        p.update(x + 2, y + 2, (4, 4));
        assert!(!p.live);
        assert_eq!(tab_release(p, &geo, strip, x + 2, y + 2), TabRelease::Pick(0));
        assert_eq!(tab_release(p, &geo, strip, geo.tabs[1].left + 10, y), TabRelease::None);
        p.update(geo.tabs[2].right - 1, y, (4, 4));
        assert!(p.live);
        p.slot = tab_drop_slot(&geo, strip, p.x, p.y);
        assert_eq!(p.slot, Some(3));
        assert_eq!(tab_release(p, &geo, strip, p.x, p.y), TabRelease::Move(0, 3));
        let marker = tab_drag_marker(&geo, strip, Some(p), 96).unwrap();
        assert!(marker.left >= strip.left && marker.right <= strip.right && marker.top >= strip.top);
        assert_eq!(tab_release(p, &geo, strip, p.x, strip.bottom), TabRelease::None);
        assert_eq!(tab_drag_marker(&geo, strip, None, 96), None);
        p.slot = Some(1); // either side of itself is a no-op
        assert_eq!(tab_drag_marker(&geo, strip, Some(p), 96), None);
        p.update(x, y, (4, 4));
        assert!(p.live); // returning to the origin does not turn a drag into a click
    }

    #[test]
    fn overflow_drag_reaches_hidden_tabs_at_each_scale() {
        for dpi in [96, 144, 192] {
            let s = |v: i32| v * dpi / 96;
            let strip = RECT { left: s(2), top: s(30), right: s(402), bottom: s(56) };
            let mut p = pressed(0, s(20), s(40));
            p.live = true;
            for first in 0..=7 {
                p.first = first;
                let geo = tab_drag_geo(strip, 10, 0, dpi, Some(p));
                assert_eq!(geo.first, first);
                let (x, y) = (geo.tabs.last().unwrap().right - 1, s(40));
                assert_eq!(tab_scroll_dir(&geo, strip, 10, x, y, dpi), if first < 7 { 1 } else { 0 });
                assert_eq!(tab_scroll_dir(&geo, strip, 10, geo.tabs[0].left, y, dpi), if first > 0 { -1 } else { 0 });
                assert_eq!(tab_drop_slot(&geo, strip, x, y), Some(first + 3));
                assert_eq!(tab_scroll_dir(&geo, strip, 10, x, strip.bottom, dpi), 0);
                assert_eq!(tab_drop_slot(&geo, strip, x, strip.bottom), None);
            }
            let geo = tab_drag_geo(strip, 10, 0, dpi, Some(p));
            assert_eq!(tab_release(p, &geo, strip, geo.tabs.last().unwrap().right - 1, s(40)), TabRelease::Move(0, 10));
            assert_eq!(tab_drag_geo(strip, 10, 0, dpi, None).first, 0); // cancel restores active-tab visibility
        }
    }

    #[test]
    fn bands_render_offscreen_without_close_buttons() {
        // Paint to a memory bitmap: no app window, mouse input or desktop capture.
        unsafe {
            let dc = CreateCompatibleDC(None);
            assert!(!dc.0.is_null());
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: 402, biHeight: -80, biPlanes: 1, biBitCount: 32, ..Default::default() },
                ..Default::default()
            };
            let mut bits = std::ptr::null_mut();
            let bitmap = CreateDIBSection(Some(dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap();
            let old = SelectObject(dc, bitmap.into());
            let font = HFONT(GetStockObject(DEFAULT_GUI_FONT).0);
            let mut pg = pane_geo(402, 80, 96);
            pg.tabs = RECT { left: 2, top: 30, right: 400, bottom: 56 };
            let colors = [0, 2];
            let geo = tab_rects(pg.tabs, 2, 0, 96);
            paint_tabs(dc, &pg, &["A".into(), "B".into()], &colors, 0, 96, font, None);
            for (k, r) in geo.tabs.iter().enumerate() {
                let color = pal().accents[colors[k] as usize].1;
                assert_eq!(GetPixel(dc, r.left + 10, r.top + 1), cref(color));
                let bg = if k == 0 { blend(pal().card, color, 0.12) } else { pal().bg };
                assert_eq!(GetPixel(dc, r.left + 10, r.top + 3), cref(if k == 0 { color } else { bg }));
                // The old close-button area is now plain tab background.
                for x in r.right - 20..r.right - 4 {
                    for y in r.top + 5..r.bottom - 2 {
                        assert_eq!(GetPixel(dc, x, y), cref(bg));
                    }
                }
            }
            SelectObject(dc, old);
            _ = DeleteObject(bitmap.into());
            _ = DeleteDC(dc);
        }
    }
}

//! A reel at one moment: which shot is showing, each item's pose, and how far through a cut it
//! is.
//!
//! Pure, and exact at any frame. Nothing is stepped: an item's pose at `t` is its placement with
//! every action due by `t` folded over it, in time order. A still at 18.5 s is the frame the video
//! has at 18.5 s, and scrubbing backwards is as cheap as forwards.

use super::file::{Action, Cut, Delta, Item, Reel, Rgba, Shot, Verb};
use crate::ease::Ease;

/// How long a cut's colour takes to rise over the end of the shot before it.
pub const CUT_LEAD: f32 = 0.1;

/// What the reel draws at one moment.
#[derive(Clone, Debug, PartialEq)]
pub struct Scene<'a> {
    /// The shot showing, by index.
    pub shot: usize,
    /// Seconds into that shot.
    pub local: f32,
    /// The shot's game viewport in points, if it has a game.
    pub game: Option<(u32, u32)>,
    /// The items showing, back first, each in its pose now.
    pub layers: Vec<Layer<'a>>,
    /// A cut's colour over everything, at its alpha now, if a cut is under way.
    pub veil: Option<Rgba>,
}

/// One item, posed.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer<'a> {
    pub item: &'a Item,
    /// Its change from where it was placed, its parents' included.
    pub pose: Delta,
}

/// Which shot is showing at `t` seconds, and how far into it. Past the end, the last shot's
/// last moment.
pub fn moment(reel: &Reel, t: f32) -> (usize, f32) {
    let mut start = 0.0;
    for (i, shot) in reel.shots.iter().enumerate() {
        if t < start + shot.seconds {
            return (i, (t - start).max(0.0));
        }
        start += shot.seconds;
    }
    let last = reel.shots.len().saturating_sub(1);
    (last, reel.shots.get(last).map_or(0.0, |s| s.seconds))
}

/// The time of output frame `frame`.
pub fn time_of(reel: &Reel, frame: u32) -> f32 {
    frame as f32 / reel.fps as f32
}

/// The reel at `t` seconds.
pub fn scene(reel: &Reel, t: f32) -> Scene<'_> {
    let (index, local) = moment(reel, t);
    let shot = &reel.shots[index];
    let layers = shot
        .items
        .iter()
        .filter_map(|item| {
            let pose = world_pose(shot, item, local, 0)?;
            (pose.alpha > 0.0).then_some(Layer { item, pose })
        })
        .collect();
    Scene {
        shot: index,
        local,
        game: shot.game.as_ref().map(|g| g.size),
        layers,
        veil: veil(reel, index, local),
    }
}

/// An item's pose with its parents': theirs and its own composed (offsets add, factors
/// multiply), and hidden while any of them is. Each turns and scales about its own anchor.
fn world_pose(shot: &Shot, item: &Item, t: f32, depth: usize) -> Option<Delta> {
    let own = match &item.name {
        Some(name) => pose(&shot.actions, name, t)?,
        None => Delta::NONE,
    };
    let Some(parent) = &item.parent else {
        return Some(own);
    };
    // A cycle of parents is a mistake the parser does not look for; stop rather than recurse.
    if depth > 16 {
        return Some(own);
    }
    let parent = shot
        .items
        .iter()
        .find(|i| i.name.as_deref() == Some(parent.as_str()))?;
    Some(world_pose(shot, parent, t, depth + 1)?.then(own))
}

/// An item's own pose at `t`: `None` while it is not showing.
///
/// It shows from the start, unless its first coming-or-going action is an `enter` or a `show`.
/// Each action due by `t` then changes it, in time order: an `enter` arrives from its preset's
/// pose, an `exit` leaves to it (and the item is gone once it has), a `move` adds its share.
pub fn pose(actions: &[Action], name: &str, t: f32) -> Option<Delta> {
    let mine = || actions.iter().filter(move |a| a.item == name);
    let arrives = mine()
        .find(|a| !matches!(a.verb, Verb::Move { .. }))
        .is_some_and(|a| matches!(a.verb, Verb::Enter { .. } | Verb::Show));
    let mut showing = !arrives;
    let mut delta = Delta::NONE;
    for action in mine().take_while(|a| a.at <= t) {
        let since = t - action.at;
        match action.verb {
            Verb::Show => showing = true,
            Verb::Hide => showing = false,
            Verb::Enter { preset, seconds } => {
                showing = true;
                let left = 1.0 - preset.ease().apply(progress(since, seconds));
                delta = delta.then(preset.away().part(left));
            }
            Verb::Exit { preset, seconds } => {
                if since >= seconds {
                    showing = false;
                } else {
                    let gone = Ease::QuadIn.apply(progress(since, seconds));
                    delta = delta.then(preset.away().part(gone));
                }
            }
            Verb::Move {
                seconds,
                by,
                ease,
                yoyo,
            } => delta = delta.then(by.part(there(since, seconds, ease, yoyo))),
        }
    }
    showing.then_some(delta)
}

/// How far through `seconds` a thing `since` seconds in is: 0 to 1, and 1 for no time at all.
fn progress(since: f32, seconds: f32) -> f32 {
    if seconds <= 0.0 {
        1.0
    } else {
        (since / seconds).clamp(0.0, 1.0)
    }
}

/// How far out a move is, `since` seconds in: each leg of `seconds` eased, the odd legs coming
/// back. After its last leg it rests where that leg ended: out after an even number of yoyos,
/// home after an odd one.
fn there(since: f32, seconds: f32, ease: Ease, yoyo: u32) -> f32 {
    let legs = yoyo + 1;
    let elapsed = since.clamp(0.0, seconds * legs as f32);
    let leg = ((elapsed / seconds) as u32).min(legs - 1);
    let f = ease.apply((elapsed - leg as f32 * seconds) / seconds);
    if leg.is_multiple_of(2) { f } else { 1.0 - f }
}

/// A cut's colour at this moment: rising over the last [`CUT_LEAD`] of the shot before a cut, and
/// falling away, eased, over the cut's own length into the shot.
fn veil(reel: &Reel, index: usize, local: f32) -> Option<Rgba> {
    let shot = &reel.shots[index];
    if let Cut::Through { colour, seconds } = shot.cut
        && index > 0
        && local < seconds
    {
        let left = 1.0 - local / seconds;
        // Quadratic out: most of the colour goes early, the rest settles.
        return Some(with_alpha(colour, left * left));
    }
    let next = reel.shots.get(index + 1)?;
    let Cut::Through { colour, .. } = next.cut else {
        return None;
    };
    let rise = (local - (shot.seconds - CUT_LEAD)) / CUT_LEAD;
    (rise > 0.0).then(|| with_alpha(colour, rise.min(1.0)))
}

fn with_alpha(c: Rgba, alpha: f32) -> Rgba {
    let [r, g, b, a] = c.0;
    Rgba([r, g, b, (a as f32 * alpha.clamp(0.0, 1.0)).round() as u8])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reel(body: &str) -> Reel {
        Reel::parse(&format!("reel 100x100 30\nink ground #070c09\n{body}")).unwrap()
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn a_moment_is_found_in_its_shot() {
        let r = reel("[a] 2\n[b] 3");
        assert_eq!(moment(&r, 0.0), (0, 0.0));
        assert_eq!(moment(&r, 2.0), (1, 0.0));
        assert_eq!(
            moment(&r, 9.0),
            (1, 3.0),
            "past the end holds the last moment"
        );
        assert_eq!(time_of(&r, 45), 1.5);
    }

    #[test]
    fn an_item_shows_from_the_start_unless_it_enters() {
        let r = reel(
            "[a] 4\nfill @bg #111\nfill @card #222\n1 enter @card fade:0.5\n3 exit @card fade:0.5",
        );
        let names = |t: f32| -> Vec<String> {
            scene(&r, t)
                .layers
                .iter()
                .filter_map(|l| l.item.name.clone())
                .collect()
        };
        assert_eq!(names(0.5), ["bg"]);
        assert_eq!(
            names(1.0),
            ["bg"],
            "an enter starts invisible: alpha 0 is not drawn"
        );
        assert_eq!(names(1.25), ["bg", "card"]);
        assert_eq!(names(3.25), ["bg", "card"]);
        assert_eq!(names(3.5), ["bg"], "gone once its exit is over");
    }

    #[test]
    fn an_enter_arrives_from_its_presets_pose() {
        let r = reel("[a] 4\nfill @t #fff\n1 enter @t rise:0.4");
        let actions = &r.shots[0].actions;
        assert_eq!(pose(actions, "t", 0.5), None);
        let start = pose(actions, "t", 1.0).unwrap();
        assert_eq!((start.y, start.alpha), (20.0, 0.0));
        let mid = pose(actions, "t", 1.2).unwrap();
        // Quad out at half way is three quarters there.
        assert!(close(mid.y, 5.0) && close(mid.alpha, 0.75), "{mid:?}");
        assert_eq!(pose(actions, "t", 2.0).unwrap(), Delta::NONE);
    }

    #[test]
    fn a_yoyo_goes_out_and_back_and_rests_where_its_last_leg_ends() {
        let r = reel("[a] 9\nfill @p #fff\n0 move @p 1 y=-100 ease=linear yoyo=2");
        let y = |t: f32| pose(&r.shots[0].actions, "p", t).unwrap().y;
        assert!(close(y(0.5), -50.0));
        assert!(close(y(1.0), -100.0) || close(y(1.0), -100.0));
        assert!(close(y(1.5), -50.0), "coming back");
        assert!(close(y(2.5), -50.0), "out again");
        assert!(close(y(8.0), -100.0), "two yoyos: three legs, ending out");
    }

    #[test]
    fn moves_add_up_and_a_drop_leaves_the_frame() {
        let r = reel(
            "[a] 9\nfill @p #fff\n0 move @p 1 x=10 ease=linear\n2 move @p 1 x=5 ease=linear\n4 exit @p drop:1",
        );
        let a = &r.shots[0].actions;
        assert!(close(pose(a, "p", 3.0).unwrap().x, 15.0));
        let falling = pose(a, "p", 4.5).unwrap();
        assert!(falling.frames > 0.0 && falling.rot > 0.0);
        assert_eq!(pose(a, "p", 5.0), None);
    }

    #[test]
    fn a_child_moves_with_its_parent_and_hides_with_it() {
        let r = reel(
            "[a] 4\nfill @ph #fff\nfill @sheet #000 in=@ph\n0 move @ph 1 y=100 ease=linear\n2 hide @ph",
        );
        let sheet = |t: f32| {
            scene(&r, t)
                .layers
                .iter()
                .find(|l| l.item.name.as_deref() == Some("sheet"))
                .map(|l| l.pose.y)
        };
        assert_eq!(sheet(1.0), Some(100.0));
        assert_eq!(sheet(2.5), None);
    }

    #[test]
    fn a_dip_rises_over_the_shot_before_and_falls_away_into_the_next() {
        let r = reel("[a] 2\n[b] 3 cut=dip:0.5:ground");
        let alpha = |t: f32| scene(&r, t).veil.map_or(0, |c| c.0[3]);
        assert_eq!(alpha(1.8), 0);
        assert!((126..=129).contains(&alpha(1.95)), "half way up the lead");
        assert_eq!(alpha(2.0), 255, "full at the cut");
        assert_eq!(
            alpha(2.25),
            64,
            "a quarter left half way through: eased out"
        );
        assert_eq!(alpha(2.5), 0);
        assert_eq!(scene(&r, 2.0).veil.unwrap().0[..3], [7, 12, 9]);
    }
}

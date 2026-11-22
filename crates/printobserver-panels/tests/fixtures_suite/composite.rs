//! The check image composed from recorded frames: its layout, what each
//! panel shows, and where panel 1 draws the planned toolpath.

use image::{Rgb, RgbImage, imageops::FilterType};
use printobserver_core::{ExtrusionKind, LayerRef, LayerToolpath, ObjectOutline};
use printobserver_panels::compose::{
    BAND_HEIGHT, IMAGE_HEIGHT, MARGIN_PX, MIN_CROP_WIDTH_PX, PANEL_HEIGHT, PANEL_WIDTH,
};
use printobserver_panels::{CameraCalibration, CheckImage, CheckImageInputs, compose_check_image};
use printobserver_types::Timestamp;

use crate::support::{
    calibration, decoded, frame, index, layer_at_z, recorded_z, snapshot_url, toolpath,
};

/// The PETG jar top.
const PETG: &str = "jar-top-petg-healthy";

/// The colour each kind is drawn in, written out here rather than read from
/// the crate so that the mapping itself is under test.
fn colour_of(kind: ExtrusionKind) -> Rgb<u8> {
    match kind {
        ExtrusionKind::Wall => Rgb([255, 40, 40]),
        ExtrusionKind::Infill => Rgb([255, 220, 0]),
        ExtrusionKind::Support => Rgb([0, 200, 255]),
        ExtrusionKind::Bridge => Rgb([255, 0, 255]),
        ExtrusionKind::Overhang => Rgb([0, 230, 0]),
        ExtrusionKind::Other => Rgb([255, 255, 255]),
    }
}

/// The grey the object layer below a support-only layer is drawn in.
const BELOW_GREY: Rgb<u8> = Rgb([150, 150, 150]);

/// A frame of it with an object layer under way.
const PETG_WALLS: &str = "agent-031-obs-125703-p43.2.jpg";

/// The frame before it among the fixtures.
const PETG_BEFORE: &str = "agent-028-obs-125100-p39.7.jpg";

/// Compose one check of a recorded frame at its recorded layer.
fn compose(
    case: &str,
    file: &str,
    previous: Option<&str>,
    outlines: Option<&[ObjectOutline]>,
) -> (CheckImage, LayerToolpath) {
    compose_at(
        case,
        file,
        previous.map(|file| (file, Timestamp::now())),
        outlines,
    )
}

/// The same, the previous frame captured at a time of the test's own.
fn compose_at(
    case: &str,
    file: &str,
    previous: Option<(&str, Timestamp)>,
    outlines: Option<&[ObjectOutline]>,
) -> (CheckImage, LayerToolpath) {
    let calibration = calibration();
    let index = index(case);
    let print_z = recorded_z(case, file);
    let layer = layer_at_z(&index, print_z);
    let path = toolpath(case, layer);
    let current = frame(case, file);
    let earlier = previous.map(|(file, at)| (frame(case, file), at));
    let url = snapshot_url();
    let image = compose_check_image(&CheckImageInputs {
        current_jpeg: &current,
        current_at: Timestamp::now(),
        camera_snapshot_url: &url,
        previous: earlier.as_ref().map(|(jpeg, at)| (jpeg.as_slice(), *at)),
        outlines: outlines.unwrap_or_else(|| index.object_outlines()),
        active_object: 0,
        toolpath: &path,
        object_layer_below: None,
        print_z_mm: print_z,
        calibration: &calibration,
    })
    .expect("the check composes");
    (image, path)
}

/// The composite, decoded.
fn decode(image: &CheckImage) -> RgbImage {
    image::load_from_memory(&image.jpeg)
        .expect("the check image decodes")
        .to_rgb8()
}

/// One panel of a decoded composite.
fn panel(composite: &RgbImage, n: u32) -> RgbImage {
    image::imageops::crop_imm(
        composite,
        0,
        n * (BAND_HEIGHT + PANEL_HEIGHT) + BAND_HEIGHT,
        PANEL_WIDTH,
        PANEL_HEIGHT,
    )
    .to_image()
}

/// A frame, or a rectangle of it, scaled to a panel as the composite scales
/// it.
fn scaled(frame: &RgbImage, rect: Option<[u32; 4]>) -> RgbImage {
    let source = rect.map_or_else(
        || frame.clone(),
        |[x, y, w, h]| image::imageops::crop_imm(frame, x, y, w, h).to_image(),
    );
    image::imageops::resize(&source, PANEL_WIDTH, PANEL_HEIGHT, FilterType::Triangle)
}

/// The mean absolute channel difference of two same-sized images.
fn mean_difference(a: &RgbImage, b: &RgbImage) -> f64 {
    let total: u32 = a
        .as_raw()
        .iter()
        .zip(b.as_raw())
        .map(|(x, y)| u32::from(x.abs_diff(*y)))
        .sum();
    f64::from(total) / f64::from(u32::try_from(a.as_raw().len()).expect("a count"))
}

/// How far one colour is from another, the largest channel difference.
fn colour_distance(a: Rgb<u8>, b: Rgb<u8>) -> u8 {
    a.0.iter()
        .zip(b.0)
        .map(|(x, y)| x.abs_diff(y))
        .max()
        .unwrap_or(0)
}

/// Whether a pixel within 1 px of a point carries a colour.
fn carries(panel: &RgbImage, at: [f64; 2], colour: Rgb<u8>, tolerance: u8) -> bool {
    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "a point inside the panel"
    )]
    let (x, y) = (at[0].round() as u32, at[1].round() as u32);
    (x.saturating_sub(1)..=(x + 1).min(PANEL_WIDTH - 1)).any(|px| {
        (y.saturating_sub(1)..=(y + 1).min(PANEL_HEIGHT - 1))
            .any(|py| colour_distance(*panel.get_pixel(px, py), colour) <= tolerance)
    })
}

/// The midpoints of a layer's segments of some kinds, projected and scaled
/// into panel 1, each with its direction there; segments shorter than 4.5
/// panel pixels are left out, as too short to sample beside, and longer than
/// 30. Both are the 6 and 40 the 1024-wide panels sampled between, scaled to
/// the 768-wide ones, so the same segments are sampled.
fn sampled(
    calibration: &CameraCalibration,
    path: &LayerToolpath,
    kinds: &[ExtrusionKind],
    print_z: f64,
) -> Vec<([f64; 2], [f64; 2])> {
    let (sx, sy) = (
        f64::from(PANEL_WIDTH) / f64::from(calibration.image_size_px[0]),
        f64::from(PANEL_HEIGHT) / f64::from(calibration.image_size_px[1]),
    );
    let h = f64::from(path.z_mm);
    path.segments
        .iter()
        .filter(|segment| kinds.contains(&segment.kind))
        .filter_map(|segment| {
            let ends = calibration.project(
                &[
                    [
                        f64::from(segment.from_mm[0]),
                        f64::from(segment.from_mm[1]),
                        h,
                    ],
                    [f64::from(segment.to_mm[0]), f64::from(segment.to_mm[1]), h],
                ],
                print_z,
                calibration.image_size_px,
            );
            let (a, b) = (ends[0]?, ends[1]?);
            let (a, b) = ([a[0] * sx, a[1] * sy], [b[0] * sx, b[1] * sy]);
            let length = (b[0] - a[0]).hypot(b[1] - a[1]);
            (4.5..30.0).contains(&length).then(|| {
                (
                    [f64::midpoint(a[0], b[0]), f64::midpoint(a[1], b[1])],
                    [(b[0] - a[0]) / length, (b[1] - a[1]) / length],
                )
            })
        })
        .collect()
}

/// Every segment of a layer projected into panel coordinates, as
/// `[x0, y0, x1, y1]`.
fn all_pieces(
    calibration: &CameraCalibration,
    path: &LayerToolpath,
    print_z: f64,
) -> Vec<[f64; 4]> {
    let (sx, sy) = (
        f64::from(PANEL_WIDTH) / f64::from(calibration.image_size_px[0]),
        f64::from(PANEL_HEIGHT) / f64::from(calibration.image_size_px[1]),
    );
    let h = f64::from(path.z_mm);
    path.segments
        .iter()
        .filter_map(|segment| {
            let ends = calibration.project(
                &[
                    [
                        f64::from(segment.from_mm[0]),
                        f64::from(segment.from_mm[1]),
                        h,
                    ],
                    [f64::from(segment.to_mm[0]), f64::from(segment.to_mm[1]), h],
                ],
                print_z,
                [100_000, 100_000],
            );
            let (a, b) = (ends[0]?, ends[1]?);
            Some([a[0] * sx, a[1] * sy, b[0] * sx, b[1] * sy])
        })
        .collect()
}

/// The share of sampled points at which panel 1 carries a colour.
fn share_carrying(panel: &RgbImage, samples: &[([f64; 2], [f64; 2])], colour: Rgb<u8>) -> f64 {
    let hits = samples
        .iter()
        .filter(|(at, _)| carries(panel, *at, colour, 60))
        .count();
    f64::from(u32::try_from(hits).expect("a count"))
        / f64::from(u32::try_from(samples.len().max(1)).expect("a count"))
}

/// A planned support and a planned bridge are each drawn in their kind's
/// colour where they project.
#[test]
fn support_and_bridge_are_drawn_in_their_colours() {
    let calibration = calibration();
    for (file, kind) in [
        ("agent-027-obs-124858-p38.9.jpg", ExtrusionKind::Support),
        ("agent-028-obs-125100-p39.7.jpg", ExtrusionKind::Bridge),
    ] {
        let (image, path) = compose(PETG, file, None, None);
        let samples = sampled(&calibration, &path, &[kind], recorded_z(PETG, file));
        assert!(samples.len() >= 10, "{file} plans {kind:?} at its layer");
        let share = share_carrying(&panel(&decode(&image), 0), &samples, colour_of(kind));
        assert!(
            share >= 0.7,
            "{file}: {:.0}% of sampled {kind:?} segments carry its colour; at least 70%",
            share * 100.0
        );
    }
}

/// On a support-only layer the nearest object layer below is drawn in grey
/// beneath, so the part still shows.
#[test]
fn a_support_only_layer_shows_the_part_below_in_grey() {
    let calibration = calibration();
    let index = index(PETG);
    let support_only = (0..index.layer_count())
        .map(LayerRef::of_index)
        .find(|layer| {
            index
                .first_object_layer()
                .is_some_and(|(first, _)| *layer > first)
                && index.object_layer_at_or_below(*layer) != Some(*layer)
        })
        .expect("the slice has a support-only layer above the first");
    let below = index
        .object_layer_at_or_below(support_only)
        .expect("a part layer below it");
    let path = toolpath(PETG, support_only);
    let under = toolpath(PETG, below);
    assert!(!path.carries_object_extrusion());
    let print_z = f64::from(path.z_mm);
    let current = frame(PETG, "agent-018-obs-123246-p24.5.jpg");
    let url = snapshot_url();
    let image = compose_check_image(&CheckImageInputs {
        current_jpeg: &current,
        current_at: Timestamp::now(),
        camera_snapshot_url: &url,
        previous: None,
        outlines: index.object_outlines(),
        active_object: 0,
        toolpath: &path,
        object_layer_below: Some(&under),
        print_z_mm: print_z,
        calibration: &calibration,
    })
    .expect("the check composes");
    let samples = sampled(&calibration, &under, &[ExtrusionKind::Wall], print_z);
    assert!(samples.len() >= 10);
    let share = share_carrying(&panel(&decode(&image), 0), &samples, BELOW_GREY);
    assert!(
        share >= 0.5,
        "{:.0}% of the layer below's sampled walls carry its grey; at least 50%",
        share * 100.0
    );
}

/// Infill is not drawn: where the layer's infill projects, panel 1 is the
/// plain frame rather than the infill colour, so the part's interior — or bare
/// sheet where a part has left its place — shows; panel 1's band keys no
/// infill colour; and the layer's walls are still drawn.
#[test]
fn infill_is_never_drawn_over_the_part() {
    let calibration = calibration();
    let (image, path) = compose(PETG, PETG_WALLS, None, None);
    let print_z = recorded_z(PETG, PETG_WALLS);
    let composite = decode(&image);
    let first = panel(&composite, 0);
    let infill = sampled(&calibration, &path, &[ExtrusionKind::Infill], print_z);
    assert!(infill.len() >= 20, "the layer lays infill to sample");
    let yellow = Rgb([255, 220, 0]);
    let share = share_carrying(&first, &infill, yellow);
    assert!(
        share <= 0.05,
        "{:.0}% of sampled infill carries the infill colour; none is drawn",
        share * 100.0
    );
    let keyed = (0..BAND_HEIGHT)
        .flat_map(|y| (0..PANEL_WIDTH).map(move |x| (x, y)))
        .filter(|(x, y)| colour_distance(*composite.get_pixel(*x, *y), yellow) <= 40)
        .count();
    assert!(
        keyed < 20,
        "band 1 keys infill ({keyed} pixels of its colour)"
    );
    let walls = sampled(&calibration, &path, &[ExtrusionKind::Wall], print_z);
    assert!(
        share_carrying(&first, &walls, colour_of(ExtrusionKind::Wall)) >= 0.7,
        "the walls are still drawn"
    );
    assert!(!printobserver_panels::is_drawn(ExtrusionKind::Infill));
}

/// Walls are drawn in the wall colour where they project, 1 px wide: 2 px to
/// either side, where no projected segment passes, the panel is the plain
/// scaled frame within JPEG noise.
#[test]
fn walls_are_drawn_where_they_project_and_one_pixel_wide() {
    let calibration = calibration();
    let (image, path) = compose(PETG, PETG_WALLS, None, None);
    let print_z = recorded_z(PETG, PETG_WALLS);
    let first = panel(&decode(&image), 0);
    let plain = scaled(&decoded(PETG, PETG_WALLS), None);
    let walls = sampled(&calibration, &path, &[ExtrusionKind::Wall], print_z);
    assert!(walls.len() >= 20);
    let share = share_carrying(&first, &walls, colour_of(ExtrusionKind::Wall));
    assert!(
        share >= 0.7,
        "{:.0}% of sampled walls carry the wall colour",
        share * 100.0
    );

    let pieces = all_pieces(&calibration, &path, print_z);
    let clear = |at: [f64; 2]| {
        pieces.iter().all(|[x0, y0, x1, y1]| {
            let (dx, dy) = (x1 - x0, y1 - y0);
            let length = (dx * dx + dy * dy).max(1e-9);
            let t = (((at[0] - x0) * dx + (at[1] - y0) * dy) / length).clamp(0.0, 1.0);
            (at[0] - (x0 + t * dx)).hypot(at[1] - (y0 + t * dy)) > 1.5
        })
    };
    let (mut beside, mut plain_beside) = (0, 0);
    for (at, direction) in &walls {
        for side in [-2.0, 2.0] {
            let point = [at[0] - direction[1] * side, at[1] + direction[0] * side];
            if !clear(point) {
                continue;
            }
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a point inside the panel"
            )]
            let (x, y) = (point[0].round() as u32, point[1].round() as u32);
            if x >= PANEL_WIDTH || y >= PANEL_HEIGHT {
                continue;
            }
            beside += 1;
            if colour_distance(*first.get_pixel(x, y), *plain.get_pixel(x, y)) <= 40 {
                plain_beside += 1;
            }
        }
    }
    assert!(
        beside >= 20,
        "enough points beside the walls are clear of every segment"
    );
    let share = f64::from(plain_beside) / f64::from(beside);
    assert!(
        share >= 0.85,
        "{:.0}% of points 2 px beside a wall are the plain frame; at least 85%",
        share * 100.0
    );
}

/// The colours a segment is drawn in, which panels 2 and 3 never carry.
fn drawn_colours() -> Vec<Rgb<u8>> {
    [
        ExtrusionKind::Wall,
        ExtrusionKind::Support,
        ExtrusionKind::Bridge,
        ExtrusionKind::Overhang,
    ]
    .into_iter()
    .map(colour_of)
    .collect()
}

/// How many pixels of a panel are within 30 of a drawn colour.
fn drawn_pixels(panel: &RgbImage) -> usize {
    let colours = drawn_colours();
    panel
        .pixels()
        .filter(|pixel| {
            colours
                .iter()
                .any(|colour| colour_distance(**pixel, *colour) <= 30)
        })
        .count()
}

/// The composite is 768x1392: three panels under three label bands that
/// each carry text, panels 2 and 3 carrying no drawn colour; panel 2 is the
/// frame cropped to the reported rectangle and not the whole frame; panel 3
/// is the previous frame scaled whole, or on a first check the placeholder.
#[test]
fn the_composite_is_three_labelled_panels() {
    let (image, _) = compose(PETG, PETG_WALLS, Some(PETG_BEFORE), None);
    assert_eq!((image.width, image.height), (768, 1392));
    let composite = decode(&image);
    assert_eq!(composite.dimensions(), (PANEL_WIDTH, IMAGE_HEIGHT));
    for n in 0..3 {
        let top = n * (BAND_HEIGHT + PANEL_HEIGHT);
        let text = (top..top + BAND_HEIGHT)
            .flat_map(|y| (0..PANEL_WIDTH).map(move |x| (x, y)))
            .filter(|(x, y)| composite.get_pixel(*x, *y).0[0] > 150)
            .count();
        assert!(
            text > 200,
            "band {} carries text ({text} bright pixels)",
            n + 1
        );
    }
    let current = decoded(PETG, PETG_WALLS);
    let first = panel(&composite, 0);
    let second = panel(&composite, 1);
    let third = panel(&composite, 2);
    let total = (PANEL_WIDTH * PANEL_HEIGHT) as usize;
    assert!(drawn_pixels(&first) > 1000, "panel 1 is drawn on");
    for (name, panel) in [("2", &second), ("3", &third)] {
        let drawn = drawn_pixels(panel);
        assert!(
            drawn * 2000 < total,
            "panel {name} carries {drawn} pixels of a drawn colour"
        );
    }
    let crop = scaled(&current, Some(image.close_up_source_rect));
    let whole = scaled(&current, None);
    let to_crop = mean_difference(&second, &crop);
    let to_whole = mean_difference(&second, &whole);
    assert!(
        to_crop < 4.0,
        "panel 2 is the crop, within JPEG noise ({to_crop:.2})"
    );
    assert!(
        to_whole > 10.0,
        "panel 2 is not the whole frame ({to_whole:.2})"
    );
    let previous = scaled(&decoded(PETG, PETG_BEFORE), None);
    let to_previous = mean_difference(&third, &previous);
    assert!(
        to_previous < 4.0,
        "panel 3 is the previous frame ({to_previous:.2})"
    );

    let (first_check, _) = compose(PETG, PETG_WALLS, None, None);
    let placeholder = panel(&decode(&first_check), 2);
    let grey = placeholder
        .pixels()
        .filter(|pixel| colour_distance(**pixel, Rgb([48, 48, 48])) <= 12)
        .count();
    let text = placeholder
        .pixels()
        .filter(|pixel| pixel.0[0] > 150)
        .count();
    assert!(
        grey * 10 > total * 9,
        "the first check's panel 3 is the placeholder"
    );
    assert!(
        text > 200,
        "the placeholder reads as text ({text} bright pixels)"
    );
}

/// Every vertex of an outline, projected at the bed and at the print height.
fn outline_pixels(
    calibration: &CameraCalibration,
    outline: &ObjectOutline,
    print_z: f64,
) -> Vec<[f64; 2]> {
    let points: Vec<[f64; 3]> = outline
        .hull_mm
        .iter()
        .flat_map(|[x, y]| {
            [
                [f64::from(*x), f64::from(*y), 0.0],
                [f64::from(*x), f64::from(*y), print_z],
            ]
        })
        .collect();
    calibration
        .project(&points, print_z, [100_000, 100_000])
        .into_iter()
        .flatten()
        .collect()
}

/// Whether a rectangle holds a point with the margin around it, as far as the
/// frame allows.
fn holds(rect: [u32; 4], point: [f64; 2], size: [u32; 2]) -> bool {
    let [x, y, w, h] = rect.map(f64::from);
    let (fw, fh) = (f64::from(size[0]), f64::from(size[1]));
    (point[0] - MARGIN_PX).max(0.0) >= x - 0.5
        && (point[0] + MARGIN_PX).min(fw) <= x + w + 0.5
        && (point[1] - MARGIN_PX).max(0.0) >= y - 0.5
        && (point[1] + MARGIN_PX).min(fh) <= y + h + 0.5
}

/// The close-up's shape: 16:9 and at least the least width.
fn shaped(rect: [u32; 4]) {
    let [_, _, w, h] = rect;
    assert!(
        f64::from(w) >= MIN_CROP_WIDTH_PX,
        "{rect:?} is at least 512 px wide"
    );
    assert!(
        (i64::from(w) * 9 - i64::from(h) * 16).abs() <= 16,
        "{rect:?} is 16:9"
    );
}

/// The close-up holds the active object's outline at the bed and at the
/// print height with its margin, 16:9 and at least 512 px wide — on the PETG
/// jar top and on the bubbler plate, each one G-code object.
#[test]
fn the_close_up_holds_the_active_objects_outline() {
    let calibration = calibration();
    for (case, file) in [
        (PETG, PETG_WALLS),
        (
            "bubbler-guard-and-pad-pla-healthy",
            "agent-016-obs-170731-p13.8.jpg",
        ),
    ] {
        let index = index(case);
        assert_eq!(index.object_outlines().len(), 1, "{case} labels one object");
        let (image, _) = compose(case, file, None, None);
        let rect = image.close_up_source_rect;
        shaped(rect);
        for point in outline_pixels(
            &calibration,
            &index.object_outlines()[0],
            recorded_z(case, file),
        ) {
            assert!(
                holds(rect, point, calibration.image_size_px),
                "{case}: {rect:?} holds {point:?} with its margin"
            );
        }
    }
}

/// With two objects, the close-up is the active one's and not the box of
/// every outline. No recorded case with a calibrated frame labels several
/// objects (the seven- and ten-object plates were recorded at 1280x720 on
/// other dates), so the second object is the jar top's own outline moved
/// 90 mm along X on the same frame.
#[test]
fn the_close_up_is_the_active_objects_not_every_objects() {
    let calibration = calibration();
    let index = index(PETG);
    let own = index.object_outlines()[0].clone();
    let moved = ObjectOutline {
        hull_mm: own.hull_mm.iter().map(|[x, y]| [x - 90.0, *y]).collect(),
    };
    let outlines = vec![own.clone(), moved.clone()];
    let (image, _) = compose(PETG, PETG_WALLS, None, Some(&outlines));
    let rect = image.close_up_source_rect;
    let print_z = recorded_z(PETG, PETG_WALLS);
    shaped(rect);
    for point in outline_pixels(&calibration, &own, print_z) {
        assert!(
            holds(rect, point, calibration.image_size_px),
            "{rect:?} holds the active outline"
        );
    }
    let other = outline_pixels(&calibration, &moved, print_z);
    assert!(
        other
            .iter()
            .any(|point| !holds(rect, *point, calibration.image_size_px)),
        "{rect:?} is not the box holding every outline"
    );
}

/// Panel 1's band carries the colour key, a square of each kind's colour;
/// and panel 3's label carries the previous frame's capture time, so frames
/// captured at different times are labelled apart and one time alike.
#[test]
fn the_bands_carry_the_colour_key_and_the_previous_checks_time() {
    let (image, _) = compose(PETG, PETG_WALLS, None, None);
    let composite = decode(&image);
    for kind in [
        ExtrusionKind::Wall,
        ExtrusionKind::Support,
        ExtrusionKind::Bridge,
        ExtrusionKind::Overhang,
    ] {
        let colour = colour_of(kind);
        let square = (0..BAND_HEIGHT)
            .flat_map(|y| (0..PANEL_WIDTH).map(move |x| (x, y)))
            .filter(|(x, y)| colour_distance(*composite.get_pixel(*x, *y), colour) <= 40)
            .count();
        assert!(
            square >= 60,
            "band 1 keys {kind:?} ({square} pixels of its colour)"
        );
    }
    let band = |image: &CheckImage| {
        let composite = decode(image);
        let top = 2 * (BAND_HEIGHT + PANEL_HEIGHT);
        image::imageops::crop_imm(&composite, 0, top, PANEL_WIDTH, BAND_HEIGHT).to_image()
    };
    let at = |seconds_ago: i64| {
        Timestamp::now()
            .plus_seconds(-seconds_ago)
            .expect("an instant")
    };
    // Four hours, eleven minutes and eleven seconds apart, so the hour, the
    // minute and the second each read differently whatever the host's zone.
    let morning = at(5 * 3600);
    let later_at = at(5 * 3600 - (4 * 3600 + 11 * 60 + 11));
    let (first, _) = compose_at(PETG, PETG_WALLS, Some((PETG_BEFORE, morning)), None);
    let (again, _) = compose_at(PETG, PETG_WALLS, Some((PETG_BEFORE, morning)), None);
    let (later, _) = compose_at(PETG, PETG_WALLS, Some((PETG_BEFORE, later_at)), None);
    let same = pixels_apart(&band(&first), &band(&again));
    assert_eq!(same, 0, "one time, one label ({same} pixels apart)");
    let apart = pixels_apart(&band(&first), &band(&later));
    assert!(
        apart >= GLYPH_PIXELS,
        "a frame captured four hours later is labelled with its own time \
         ({apart} pixels apart, at least {GLYPH_PIXELS} owed)"
    );
}

/// How many pixels a changed digit of a label moves at the least: a glyph's
/// strokes, well clear of the JPEG ringing beside them.
const GLYPH_PIXELS: usize = 40;

/// How many pixels of two label bands differ as a glyph's stroke does from the
/// band behind it, rather than as JPEG noise does.
fn pixels_apart(a: &RgbImage, b: &RgbImage) -> usize {
    a.pixels()
        .zip(b.pixels())
        .filter(|(x, y)| colour_distance(**x, **y) > 96)
        .count()
}

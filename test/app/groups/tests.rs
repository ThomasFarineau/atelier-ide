use super::*;

#[test]
fn the_side_under_the_pointer_is_the_middle_or_the_nearest_edge() {
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 100.0));
    assert_eq!(Side::at(rect, pos2(50.0, 50.0)), Side::Center);
    assert_eq!(Side::at(rect, pos2(95.0, 50.0)), Side::Right);
    assert_eq!(Side::at(rect, pos2(5.0, 40.0)), Side::Left);
    assert_eq!(Side::at(rect, pos2(50.0, 95.0)), Side::Bottom);
    assert_eq!(Side::at(rect, pos2(40.0, 4.0)), Side::Top);
}

#[test]
fn a_dropped_tab_takes_the_half_of_its_side() {
    let rect = Rect::from_min_size(pos2(0.0, 0.0), vec2(100.0, 80.0));
    assert_eq!(
        Side::Right.area(rect),
        Rect::from_min_max(pos2(50.0, 0.0), pos2(100.0, 80.0))
    );
    assert_eq!(
        Side::Top.area(rect),
        Rect::from_min_max(pos2(0.0, 0.0), pos2(100.0, 40.0))
    );
    assert_eq!(Side::Center.area(rect), rect);
}

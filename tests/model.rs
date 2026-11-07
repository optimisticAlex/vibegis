use vibegis::model::*;

fn sample() -> Store {
    import_csv(b"id,status,longitude,latitude\nn1,rot,12,49\nw1,gelb,12.1,49.1\nr1,gruen,13,50\n")
        .unwrap()
}

#[test]
fn selection_includes_boundary_but_not_bbox_only_matches() {
    let data = sample();
    let selected = data
        .select(&[[12., 49.], [12.2, 49.], [12., 49.2]])
        .unwrap();
    assert!(selected.contains(&0));
    assert!(!selected.contains(&2));
    let selected = data
        .select(&[[11.99, 48.99], [12.02, 48.99], [11.99, 49.02]])
        .unwrap();
    assert_eq!(selected, vec![0]);
}

#[test]
fn invalid_polygons_are_rejected() {
    assert!(selection_polygon(&[[0., 0.], [1., 1.]]).is_err());
    assert!(selection_polygon(&[[0., 0.], [1., 1.], [0., 1.], [1., 0.]]).is_err());
    assert!(selection_polygon(&[[0., 0.], [1., 0.], [2., 0.]]).is_err());
    assert!(selection_polygon(&[[0., 0.], [0., 0.], [1., 1.]]).is_err());
}

#[test]
fn csv_rejects_invalid_data_and_duplicate_ids() {
    for csv in [
        "id,status,longitude,latitude\nn1,blau,12,49\n",
        "id,status,longitude,latitude\nn1,rot,NaN,49\n",
        "id,status,longitude,latitude\nn1,rot,12,91\n",
        "id,status,longitude,latitude\nn1,rot,12,49\nn1,gelb,12,49\n",
        "id,status,longitude,latitude\n",
        "id,status,x,y\nn1,rot,12,49\n",
    ] {
        assert!(import_csv(csv.as_bytes()).is_err(), "{csv}");
    }
}

#[test]
fn status_change_undo_and_export_round_trip() {
    let mut data = sample();
    let original = data.totals;
    let undo = data.change(&[0, 1], Status::Green);
    assert_eq!(data.totals, [0, 0, 3]);
    let mut writer = csv::Writer::from_writer(Vec::new());
    writer.write_record(HEADER).unwrap();
    for house in &data.houses {
        writer.write_record(house.record()).unwrap();
    }
    let copy = import_csv(&writer.into_inner().unwrap()).unwrap();
    assert_eq!(copy.totals, data.totals);
    assert_eq!(copy.houses[1].id, data.houses[1].id);
    assert_eq!(copy.houses[1].position, data.houses[1].position);
    data.undo(&undo);
    assert_eq!(data.totals, original);
    let grid_totals = data.overview.values().fold([0usize; 3], |mut sum, c| {
        for i in 0..3 {
            sum[i] += c[i] as usize;
        }
        sum
    });
    assert_eq!(grid_totals, original);
}

#[test]
fn osm_ids_preserve_element_type() {
    for id in ["123", "n123", "w123", "r123"] {
        assert_eq!(format_id(parse_id(id).unwrap()), id);
    }
    assert_ne!(parse_id("n123").unwrap(), parse_id("w123").unwrap());
    assert!(parse_id("-1").is_err());
    assert!(parse_id("n18446744073709551615").is_err());
}

#[test]
fn spatial_queries_match_brute_force_across_cells_and_negative_coordinates() {
    let mut data = Store::default();
    for i in 0..2000 {
        data.push(House {
            id: i,
            position: [(i % 100) as f64 / 50. - 1., (i / 100) as f64 / 50. - 0.2],
            status: Status::Red,
        })
        .unwrap();
    }
    for (min, max) in [
        ([-0.15, -0.1], [0.15, 0.1]),
        ([-1., -0.2], [1., 0.2]),
        ([0., 0.], [0., 0.]),
        ([5., 5.], [6., 6.]),
        ([-180., -85.], [180., 85.]),
    ] {
        let mut indexed: Vec<_> = data.candidates(min, max).collect();
        indexed.sort_unstable();
        let expected: Vec<_> = data
            .houses
            .iter()
            .enumerate()
            .filter(|(_, h)| {
                h.position[0] >= min[0]
                    && h.position[0] <= max[0]
                    && h.position[1] >= min[1]
                    && h.position[1] <= max[1]
            })
            .map(|(i, _)| i as u32)
            .collect();
        assert_eq!(indexed, expected);
    }
}

#[test]
fn concave_selection_excludes_points_inside_bbox_but_outside_polygon() {
    let mut data = Store::default();
    for (id, position) in [[0.25, 0.75], [0.75, 0.75], [0., 0.]]
        .into_iter()
        .enumerate()
    {
        data.push(House {
            id: id as u64,
            position,
            status: Status::Red,
        })
        .unwrap();
    }
    let mut selected = data
        .select(&[
            [0., 0.],
            [1., 0.],
            [1., 0.4],
            [0.4, 0.4],
            [0.4, 1.],
            [0., 1.],
        ])
        .unwrap();
    selected.sort_unstable();
    assert_eq!(selected, vec![0, 2]);
}

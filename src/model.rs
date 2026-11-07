use geo::{Intersects, Line, LineString, Point, Polygon};
use std::collections::{HashMap, HashSet};

pub const HEADER: [&str; 4] = ["id", "status", "longitude", "latitude"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Status {
    Red,
    Yellow,
    Green,
}
impl Status {
    pub const ALL: [Self; 3] = [Self::Red, Self::Yellow, Self::Green];
    pub fn name(self) -> &'static str {
        match self {
            Self::Red => "Rot",
            Self::Yellow => "Gelb",
            Self::Green => "Grün",
        }
    }
    pub fn csv(self) -> &'static str {
        match self {
            Self::Red => "rot",
            Self::Yellow => "gelb",
            Self::Green => "gruen",
        }
    }
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_lowercase().as_str() {
            "rot" | "red" => Ok(Self::Red),
            "gelb" | "yellow" => Ok(Self::Yellow),
            "grün" | "gruen" | "green" => Ok(Self::Green),
            _ => Err(format!("Unbekannter Status: {s}")),
        }
    }
}

// Two high bits encode OSM type; IDs remain lossless across CSV round trips.
pub fn parse_id(value: &str) -> Result<u64, String> {
    let (kind, number) = match value.as_bytes().first() {
        Some(b'n') => (1_u64, &value[1..]),
        Some(b'w') => (2, &value[1..]),
        Some(b'r') => (3, &value[1..]),
        _ => (0, value),
    };
    let number: u64 = number
        .parse()
        .map_err(|_| format!("Ungültige ID: {value}"))?;
    if number >= (1_u64 << 62) {
        return Err("ID ist zu groß".into());
    }
    Ok((kind << 62) | number)
}
pub fn format_id(id: u64) -> String {
    format!(
        "{}{}",
        ["", "n", "w", "r"][(id >> 62) as usize],
        id & ((1_u64 << 62) - 1)
    )
}

#[derive(Clone, Copy, Debug)]
pub struct House {
    pub id: u64,
    pub position: [f64; 2],
    pub status: Status,
}
impl House {
    pub fn from_record(record: &csv::StringRecord) -> Result<Self, String> {
        Self::from_fields(record.iter())
    }
    pub fn from_fields<'a>(mut fields: impl Iterator<Item = &'a str>) -> Result<Self, String> {
        let mut values = [""; 4];
        for value in &mut values {
            *value = fields
                .next()
                .ok_or("Erwartet: id,status,longitude,latitude")?;
        }
        if fields.next().is_some() {
            return Err("Erwartet: id,status,longitude,latitude".into());
        }
        let x: f64 = values[2]
            .trim()
            .parse()
            .map_err(|_| "Ungültiger Längengrad")?;
        let y: f64 = values[3]
            .trim()
            .parse()
            .map_err(|_| "Ungültiger Breitengrad")?;
        if !x.is_finite()
            || !y.is_finite()
            || !(-180.0..=180.0).contains(&x)
            || !(-85.0..=85.0).contains(&y)
        {
            return Err("Koordinaten außerhalb des unterstützten WGS84-Bereichs".into());
        }
        Ok(Self {
            id: parse_id(values[0].trim())?,
            position: [x, y],
            status: Status::parse(values[1])?,
        })
    }
    pub fn record(self) -> [String; 4] {
        [
            format_id(self.id),
            self.status.csv().into(),
            self.position[0].to_string(),
            self.position[1].to_string(),
        ]
    }
}

#[derive(Default)]
pub struct Store {
    pub houses: Vec<House>,
    // Coordinates are immutable in this MVP. Each spatial cell stores compact
    // row indices only, avoiding a second copy of every point's coordinates.
    grid: HashMap<(i32, i32), Vec<u32>>,
    pub totals: [usize; 3],
    pub overview: HashMap<(i32, i32), [u32; 3]>,
    pub revision: u64,
    pub extent: Option<([f64; 2], [f64; 2])>,
}
pub fn cell(p: [f64; 2]) -> (i32, i32) {
    ((p[0] * 20.).floor() as i32, (p[1] * 20.).floor() as i32)
}
impl Store {
    pub fn push(&mut self, house: House) -> Result<(), String> {
        if let Some((min, max)) = &mut self.extent {
            for axis in 0..2 {
                min[axis] = min[axis].min(house.position[axis]);
                max[axis] = max[axis].max(house.position[axis]);
            }
        } else {
            self.extent = Some((house.position, house.position));
        }
        let index = u32::try_from(self.houses.len()).map_err(|_| "Zu viele Punkte")?;
        self.grid
            .entry(cell(house.position))
            .or_default()
            .push(index);
        self.totals[house.status as usize] += 1;
        self.overview.entry(cell(house.position)).or_default()[house.status as usize] += 1;
        self.houses.push(house);
        Ok(())
    }
    pub fn candidates(&self, min: [f64; 2], max: [f64; 2]) -> impl Iterator<Item = u32> + '_ {
        let (x0, y0) = cell(min);
        let (x1, y1) = cell(max);
        let span = (i64::from(x1) - i64::from(x0) + 1)
            .max(0)
            .saturating_mul((i64::from(y1) - i64::from(y0) + 1).max(0));
        let cells: Box<dyn Iterator<Item = &Vec<u32>> + '_> = if span > self.grid.len() as i64 {
            Box::new(
                self.grid
                    .iter()
                    .filter(move |((x, y), _)| *x >= x0 && *x <= x1 && *y >= y0 && *y <= y1)
                    .map(|(_, ids)| ids),
            )
        } else {
            Box::new(
                (x0..=x1).flat_map(move |x| (y0..=y1).filter_map(move |y| self.grid.get(&(x, y)))),
            )
        };
        cells.flat_map(|ids| ids.iter().copied()).filter(move |&i| {
            let p = self.houses[i as usize].position;
            p[0] >= min[0] && p[0] <= max[0] && p[1] >= min[1] && p[1] <= max[1]
        })
    }
    pub fn select(&self, vertices: &[[f64; 2]]) -> Result<Vec<u32>, String> {
        let polygon = selection_polygon(vertices)?;
        let (min, max) = bounds(vertices);
        Ok(self
            .candidates(min, max)
            .filter(|&i| {
                let p = mercator(self.houses[i as usize].position);
                polygon.intersects(&Point::new(p[0], p[1]))
            })
            .collect())
    }
    pub fn change(&mut self, indices: &[u32], status: Status) -> Vec<(u32, Status)> {
        let mut undo = Vec::new();
        for &i in indices {
            if let Some(house) = self.houses.get_mut(i as usize)
                && house.status != status
            {
                undo.push((i, house.status));
                self.totals[house.status as usize] -= 1;
                self.totals[status as usize] += 1;
                let counts = self.overview.get_mut(&cell(house.position)).unwrap();
                counts[house.status as usize] -= 1;
                counts[status as usize] += 1;
                house.status = status;
            }
        }
        self.revision += 1;
        undo
    }
    pub fn undo(&mut self, changes: &[(u32, Status)]) {
        for &(i, status) in changes {
            self.change(&[i], status);
        }
    }
}

pub fn bounds(points: &[[f64; 2]]) -> ([f64; 2], [f64; 2]) {
    let mut min = [f64::INFINITY; 2];
    let mut max = [f64::NEG_INFINITY; 2];
    for p in points {
        for axis in 0..2 {
            min[axis] = min[axis].min(p[axis]);
            max[axis] = max[axis].max(p[axis]);
        }
    }
    (min, max)
}
pub fn mercator(p: [f64; 2]) -> [f64; 2] {
    [
        p[0].to_radians(),
        (std::f64::consts::FRAC_PI_4 + p[1].to_radians() / 2.)
            .tan()
            .ln(),
    ]
}
pub fn selection_polygon(vertices: &[[f64; 2]]) -> Result<Polygon, String> {
    if vertices.len() < 3 {
        return Err("Mindestens drei Eckpunkte setzen.".into());
    }
    let mut points: Vec<_> = vertices.iter().copied().map(mercator).collect();
    if points.iter().flatten().any(|x| !x.is_finite()) {
        return Err("Ungültige Polygonkoordinaten.".into());
    }
    let n = points.len();
    for i in 0..n {
        if points[i] == points[(i + 1) % n] {
            return Err("Doppelte Eckpunkte entfernen.".into());
        }
        for j in (i + 1)..n {
            if j == i + 1 || (i == 0 && j == n - 1) {
                continue;
            }
            if Line::new(points[i], points[(i + 1) % n])
                .intersects(&Line::new(points[j], points[(j + 1) % n]))
            {
                return Err("Das Polygon darf sich nicht selbst schneiden.".into());
            }
        }
    }
    let area: f64 = (0..n)
        .map(|i| points[i][0] * points[(i + 1) % n][1] - points[(i + 1) % n][0] * points[i][1])
        .sum();
    if area.abs() < 1e-16 {
        return Err("Das Polygon hat keine Fläche.".into());
    }
    points.push(points[0]);
    Ok(Polygon::new(LineString::from(points), vec![]))
}

pub fn import_csv(bytes: &[u8]) -> Result<Store, String> {
    let mut reader = csv::Reader::from_reader(bytes);
    if reader
        .headers()
        .map_err(|e| e.to_string())?
        .iter()
        .ne(HEADER)
    {
        return Err("CSV-Kopf muss id,status,longitude,latitude lauten.".into());
    }
    let mut store = Store::default();
    let mut seen = HashSet::new();
    for (row, record) in reader.records().enumerate() {
        let house = House::from_record(&record.map_err(|e| e.to_string())?)
            .map_err(|e| format!("Zeile {}: {e}", row + 2))?;
        if !seen.insert(house.id) {
            return Err(format!("Doppelte ID in Zeile {}", row + 2));
        }
        store.push(house)?;
    }
    if store.houses.is_empty() {
        return Err("Die CSV enthält keine Punkte.".into());
    }
    Ok(store)
}

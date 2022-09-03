use crate::{
    candidate_index::CandidateIndex, cell_index::CellIndex, cell_utility::CellUtility,
    constraint::Constraint, house::House, logic_result::LogicResult, math::*,
    value_mask::ValueMask,
};
use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

#[derive(Clone)]
pub struct Board {
    board: Vec<ValueMask>,
    data: Arc<BoardData>,
}

#[derive(Clone)]
pub struct BoardData {
    size: usize,
    num_cells: usize,
    num_candidates: usize,
    all_values_mask: ValueMask,
    houses: Vec<Arc<House>>,
    houses_by_cell: Vec<Vec<Arc<House>>>,
    weak_links: Vec<BTreeSet<CandidateIndex>>,
    total_weak_links: usize,
    constraints: Vec<Arc<dyn Constraint>>,
}

impl Board {
    pub fn new(size: usize, regions: &[usize], constraints: &[Arc<dyn Constraint>]) -> Board {
        let mut data = BoardData::new(size, regions, constraints);
        let elims = data.init_weak_links();

        let mut board = Board {
            board: vec![data.all_values_mask; data.num_cells],
            data: Arc::new(data),
        };

        board.clear_candidates(&elims);

        board
    }

    pub fn deep_clone(&self) -> Board {
        Board {
            board: self.board.clone(),
            data: Arc::new(BoardData::clone(&self.data)),
        }
    }

    pub fn size(&self) -> usize {
        self.data.size
    }

    pub fn num_cells(&self) -> usize {
        self.data.num_cells
    }

    pub fn num_candidates(&self) -> usize {
        self.data.num_candidates
    }

    pub fn all_values_mask(&self) -> ValueMask {
        self.data.all_values_mask
    }

    pub fn houses(&self) -> &[Arc<House>] {
        &self.data.houses
    }

    pub fn houses_for_cell(&self, cell: CellIndex) -> &[Arc<House>] {
        &self.data.houses_by_cell[cell.index()]
    }

    pub fn total_weak_links(&self) -> usize {
        self.data.total_weak_links
    }

    pub fn weak_links(&self) -> &[BTreeSet<CandidateIndex>] {
        &self.data.weak_links
    }

    pub fn constraints(&self) -> &[Arc<dyn Constraint>] {
        &self.data.constraints
    }

    pub fn cell(&self, cell: CellIndex) -> ValueMask {
        self.board[cell.index()]
    }

    pub fn has_candidate(&self, candidate: CandidateIndex) -> bool {
        let (cell, val) = candidate.cell_index_and_value();
        self.cell(cell).has(val)
    }

    pub fn clear_value(&mut self, cell: CellIndex, val: usize) -> bool {
        let cell = cell.index();
        self.board[cell] = self.board[cell].without(val);
        !self.board[cell].is_empty()
    }

    pub fn clear_candidate(&mut self, candidate: CandidateIndex) -> bool {
        let (cell, val) = candidate.cell_index_and_value();
        self.clear_value(cell, val)
    }

    pub fn clear_candidates(&mut self, candidates: &[CandidateIndex]) -> bool {
        let mut valid = true;
        for candidate in candidates {
            if !self.clear_candidate(*candidate) {
                valid = false;
            }
        }
        valid
    }

    pub fn set_solved(&mut self, cell: CellIndex, val: usize) -> bool {
        // Is this value possible?
        if !self.cell(cell).has(val) {
            return false;
        }

        // Check if already solved
        if self.board[cell.index()].is_solved() {
            return false;
        }

        // Mark as solved
        self.board[cell.index()] = self.board[cell.index()].with_only(val).solved();

        // Clone the BoardData Arc to avoid borrowing issues
        let board_data = self.data.clone();

        // Apply all weak links
        let cu = CellUtility::new(self.size());
        let set_candidate_index = cu.candidate(cell, val);
        for &elim_candidate_index in board_data.weak_links[set_candidate_index.index()].iter() {
            if !self.clear_candidate(elim_candidate_index) {
                return false;
            }
        }

        // Enforce all constraints
        for constraint in board_data.constraints.iter() {
            if constraint.enforce(self, cell, val) == LogicResult::Invalid {
                return false;
            }
        }

        true
    }

    pub fn set_mask(&mut self, cell: usize, mask: ValueMask) -> bool {
        assert!(!mask.is_solved());
        if mask.is_empty() {
            return false;
        }

        self.board[cell] = mask;
        true
    }
}

impl BoardData {
    pub fn new(size: usize, regions: &[usize], constraints: &[Arc<dyn Constraint>]) -> BoardData {
        let all_values_mask = ValueMask::from_all_values(size);
        let num_cells = size * size;
        let num_candidates = size * num_cells;
        let houses = Self::create_houses(size, regions, constraints);
        let houses_by_cell = Self::create_houses_by_cell(size, &houses);

        BoardData {
            size,
            num_cells,
            num_candidates,
            all_values_mask,
            houses,
            houses_by_cell,
            weak_links: vec![BTreeSet::new(); num_candidates],
            total_weak_links: 0,
            constraints: constraints.to_vec(),
        }
    }

    fn create_houses(
        size: usize,
        regions: &[usize],
        constraints: &[Arc<dyn Constraint>],
    ) -> Vec<Arc<House>> {
        let cu = CellUtility::new(size);
        let num_cells = size * size;
        let regions = if regions.len() == num_cells {
            regions.to_vec()
        } else {
            default_regions(size)
        };

        let mut houses: Vec<Arc<House>> = Vec::new();

        // Create a house for each row
        for row in 0..size {
            let name = format!("Row {}", row + 1);
            let mut house = Vec::new();
            for col in 0..size {
                let cell = cu.cell(row, col);
                house.push(cell);
            }
            houses.push(Arc::new(House::new(&name, &house)));
        }

        // Create a house for each column
        for col in 0..size {
            let name = format!("Column {}", col + 1);
            let mut house = Vec::new();
            for row in 0..size {
                let cell = cu.cell(row, col);
                house.push(cell);
            }
            houses.push(Arc::new(House::new(&name, &house)));
        }

        // Create a house for each region
        let mut house_for_region: HashMap<usize, Vec<CellIndex>> = HashMap::new();
        for cell in cu.all_cells() {
            let region = regions[cell.index()];
            let house = house_for_region.entry(region).or_insert(Vec::new());
            house.push(cell);
        }

        // Add any regions that are not duplicates of a row/col
        for (region, house) in house_for_region.iter() {
            if house.len() == size {
                let name = format!("Region {}", region + 1);
                let house = House::new(&name, house);
                if !houses.iter().any(|h| h.cells() == house.cells()) {
                    houses.push(Arc::new(house));
                }
            }
        }

        // Add any non-duplicate regions created by constraints
        for constraint in constraints.iter() {
            let constraint_houses = constraint.get_houses(size);
            for house in constraint_houses {
                if !houses.iter().any(|h| h.cells() == house.cells()) {
                    houses.push(Arc::new(house));
                }
            }
        }

        houses
    }

    fn create_houses_by_cell(size: usize, houses: &[Arc<House>]) -> Vec<Vec<Arc<House>>> {
        let num_cells = size * size;
        let mut houses_by_cell = Vec::new();
        for _ in 0..num_cells {
            houses_by_cell.push(Vec::new());
        }
        for house in houses {
            for cell in house.cells().iter() {
                houses_by_cell[cell.index()].push(house.clone());
            }
        }
        houses_by_cell
    }

    fn add_weak_link(&mut self, candidate1: CandidateIndex, candidate2: CandidateIndex) {
        if self.weak_links[candidate1.index()].insert(candidate2) {
            self.total_weak_links += 1;
        }
        if self.weak_links[candidate2.index()].insert(candidate1) {
            self.total_weak_links += 1;
        }
    }

    fn init_weak_links(&mut self) -> Vec<CandidateIndex> {
        self.init_sudoku_weak_links();
        self.init_constraint_weak_links()
    }

    fn init_sudoku_weak_links(&mut self) {
        let size = self.size;
        let cu = CellUtility::new(size);

        for candidate1 in cu.all_candidates() {
            let (cell1, val1) = candidate1.cell_index_and_value();

            // Add a weak link to every other candidate in the same cell
            for val2 in (val1 + 1)..=size {
                let candidate2 = cu.candidate(cell1, val2);
                self.add_weak_link(candidate1, candidate2);
            }

            // Add a weak link to every other candidate with the same value that shares a house
            for house in self.houses_by_cell[cell1.index()].clone() {
                for (cand0, cand1) in cu.candidate_pairs(house.cells()) {
                    self.add_weak_link(cand0, cand1);
                }
            }
        }
    }

    fn init_constraint_weak_links(&mut self) -> Vec<CandidateIndex> {
        let mut elims: Vec<CandidateIndex> = Vec::new();
        for constraint in self.constraints.clone() {
            let weak_links = constraint.get_weak_links(self.size);
            for (candidate0, candidate1) in weak_links {
                if candidate0 != candidate1 {
                    self.add_weak_link(candidate0, candidate1);
                } else {
                    elims.push(candidate0);
                }
            }
        }
        elims
    }
}

impl Default for Board {
    /// Create an empty board of size 9x9 with standard regions (boxes)
    /// and no additional constraints.
    fn default() -> Self {
        Board::new(9, &[], &[])
    }
}

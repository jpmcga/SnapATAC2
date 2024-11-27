use std::{io::{Read, BufRead, BufReader}, collections::{HashMap, HashSet}};
use anndata::data::CsrNonCanonical;
use bed_utils::bed::{GenomicRange, BEDLike, map::GIntervalMap, ParseError, Strand};
use anyhow::Result;
use serde::{Serialize, Deserialize};
use smallvec::{SmallVec, smallvec};

pub type CellBarcode = String;

/// Fragments from single-cell ATAC-seq experiment. Each fragment is represented
/// by a genomic coordinate, cell barcode and a integer value.
#[derive(Serialize, Deserialize, Debug, Clone)] 
pub struct Fragment {
    pub chrom: String,
    pub start: u64,
    pub end: u64,
    pub barcode: Option<CellBarcode>,
    pub count: u32,
    pub strand: Option<Strand>,
}

impl Fragment {
    pub fn new(chrom: impl Into<String>, start: u64, end: u64) -> Self {
        Self {
            chrom: chrom.into(),
            start,
            end,
            barcode: None,
            count: 1,
            strand: None,
        }
    }

    pub fn to_insertions(&self) -> SmallVec<[GenomicRange; 2]> {
        match self.strand {
            None => smallvec![
                GenomicRange::new(self.chrom.clone(), self.start, self.start + 1),
                GenomicRange::new(self.chrom.clone(), self.end - 1, self.end),
            ],
            Some(Strand::Forward) => smallvec![
                GenomicRange::new(self.chrom.clone(), self.start, self.start + 1)
            ],
            Some(Strand::Reverse) => smallvec![
                GenomicRange::new(self.chrom.clone(), self.end - 1, self.end)
            ],
        }
    }

    pub fn is_single(&self) -> bool {
        self.strand.is_some()
    }
}

impl BEDLike for Fragment {
    fn chrom(&self) -> &str { &self.chrom }
    fn set_chrom(&mut self, chrom: &str) -> &mut Self {
        self.chrom = chrom.to_string();
        self
    }
    fn start(&self) -> u64 { self.start }
    fn set_start(&mut self, start: u64) -> &mut Self {
        self.start = start;
        self
    }
    fn end(&self) -> u64 { self.end }
    fn set_end(&mut self, end: u64) -> &mut Self {
        self.end = end;
        self
    }
    fn name(&self) -> Option<&str> {
        self.barcode.as_deref()
    }
    fn score(&self) -> Option<bed_utils::bed::Score> {
        Some(self.count.try_into().unwrap())
    }
    fn strand(&self) -> Option<Strand> {
        self.strand
    }
}

impl core::fmt::Display for Fragment {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "{}\t{}\t{}\t{}\t{}",
            self.chrom(),
            self.start(),
            self.end(),
            self.barcode.as_deref().unwrap_or("."),
            self.count,
        )?;
        if let Some(strand) = self.strand() {
            write!(f, "\t{}", strand)?;
        }
        Ok(())
    }
}

impl std::str::FromStr for Fragment {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut fields = s.split('\t');
        let chrom = fields.next().ok_or(ParseError::MissingReferenceSequenceName)?.to_string();
        let start = fields.next().ok_or(ParseError::MissingStartPosition)
            .and_then(|s| lexical::parse(s).map_err(ParseError::InvalidStartPosition))?;
        let end = fields.next().ok_or(ParseError::MissingEndPosition)
            .and_then(|s| lexical::parse(s).map_err(ParseError::InvalidEndPosition))?;
        let barcode = fields
            .next()
            .ok_or(ParseError::MissingName)
            .map(|s| match s {
                "." => None,
                _ => Some(s.into()),
            })?;
        let count = fields.next().map_or(Ok(1), |s| if s == "." {
            Ok(1)
        } else {
            lexical::parse(s).map_err(ParseError::InvalidStartPosition)
        })?;
        let strand = fields.next().map_or(Ok(None), |s| if s == "." {
            Ok(None)
        } else {
            s.parse().map(Some).map_err(ParseError::InvalidStrand)
        })?;
        Ok(Fragment { chrom, start, end, barcode, count, strand })
    }
}

/// Chromatin interactions from single-cell Hi-C experiment. 
#[derive(Serialize, Deserialize, Debug)] 
pub struct Contact {
    pub chrom1: String,
    pub start1: u64,
    pub chrom2: String,
    pub start2: u64,
    pub barcode: CellBarcode,
    pub count: u32,
}

impl std::str::FromStr for Contact {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut fields = s.split('\t');
        let barcode = fields.next().ok_or(ParseError::MissingName)
            .map(|s| s.into())?;
        let chrom1 = fields.next().ok_or(ParseError::MissingReferenceSequenceName)?.to_string();
        let start1 = fields.next().ok_or(ParseError::MissingStartPosition)
            .and_then(|s| lexical::parse(s).map_err(ParseError::InvalidStartPosition))?;
        let chrom2 = fields.next().ok_or(ParseError::MissingReferenceSequenceName)?.to_string();
        let start2 = fields.next().ok_or(ParseError::MissingStartPosition)
            .and_then(|s| lexical::parse(s).map_err(ParseError::InvalidStartPosition))?;
        let count = fields.next().map_or(Ok(1), |s| if s == "." {
            Ok(1)
        } else {
            lexical::parse(s).map_err(ParseError::InvalidStartPosition)
        })?;
        Ok(Contact { barcode, chrom1, start1, chrom2, start2, count })
    }
}


#[derive(Clone, Debug, PartialEq)]
pub struct QualityControl {
    pub num_unique_fragment: u64,
    pub frac_mitochondrial: f64,
    pub frac_duplicated: f64,
}

pub(crate) struct FragmentSummary<'a> {
    pub(crate) num_unique_fragment: u64,
    num_total_fragment: u64, 
    num_mitochondrial : u64,
    mitochondrial_dna: &'a HashSet<String>,
}

impl<'a> FragmentSummary<'a> {
    pub(crate) fn new(mitochondrial_dna: &'a HashSet<String>) -> Self {
        FragmentSummary {
            num_unique_fragment: 0,
            num_total_fragment: 0,
            num_mitochondrial: 0,
            mitochondrial_dna,
        }
    }

    pub(crate) fn update(&mut self, fragment: &Fragment) {
        self.num_total_fragment += fragment.count as u64;
        if self.mitochondrial_dna.contains(fragment.chrom.as_str()) {
            self.num_mitochondrial += 1;
        } else {
            self.num_unique_fragment += 1;
        }
    }

    pub(crate) fn get_qc(self) -> QualityControl {
        let frac_duplicated = 1.0 -
            (self.num_unique_fragment + self.num_mitochondrial) as f64 /
            self.num_total_fragment as f64;
        let frac_mitochondrial = self.num_mitochondrial as f64 /
            (self.num_unique_fragment + self.num_mitochondrial) as f64;
        QualityControl {
            num_unique_fragment: self.num_unique_fragment,
            frac_mitochondrial,
            frac_duplicated,
        }
    }
}

fn moving_average(half_window: usize, arr: &[u64]) -> impl Iterator<Item = f64> + '_ {
    let n = arr.len();
    (0 .. n).map(move |i| {
        let r = i.saturating_sub(half_window) .. std::cmp::min(i + half_window + 1, n);
        let l = r.len() as f64;
        arr[r].iter().sum::<u64>() as f64 / l
    })
}

/// Read tss from a gtf or gff file. Note the returned result can potentially
/// contain redudant elements as there may be multiple transcripts for the same gene
/// in the annotation file.
pub fn read_tss<R: Read>(file: R) -> impl Iterator<Item = (String, u64, bool)> {
    let reader = BufReader::new(file);
    reader.lines().filter_map(|line| {
        let chr_idx: usize = 0;
        let type_idx: usize = 2;
        let start_idx: usize = 3;
        let end_idx: usize = 4;
        let strand_idx: usize = 6;
        let l = line.unwrap();
        if l.as_bytes()[0] as char == '#' {
            return None;
        }
        let elements: Vec<&str> = l.split('\t').collect();
        if elements[type_idx] == "transcript" {
            let chr = elements[chr_idx].to_string();
            let is_fwd = elements[strand_idx] != "-";
            let tss: u64 = 
                if is_fwd {
                    elements[start_idx].parse::<u64>().unwrap() - 1
                } else {
                    elements[end_idx].parse::<u64>().unwrap() - 1
                };
            Some((chr, tss, is_fwd))
        } else {
            None
        }
    })
}

#[derive(Debug, Clone)]
pub struct TssRegions {
    pub promoters: GIntervalMap<bool>,
    window_size: u64,
}

impl TssRegions {
    /// Create a new TssRegions from an iterator of (chr, tss, is_fwd) tuples.
    /// The promoter region is defined as |--- window_size --- TSS --- window_size ---|,
    /// a total of 2 * window_size + 1 bp.
    pub fn new<I: IntoIterator<Item = (String, u64, bool)>>(iter: I, window_size: u64) -> Self {
        let promoters = iter.into_iter().map( |(chr, tss, is_fwd)| {
            let b = GenomicRange::new(chr, tss.saturating_sub(window_size), tss + window_size + 1);
            (b, is_fwd)
        }).collect();
        Self { promoters, window_size }
    }

    pub fn len(&self) -> usize {
        2 * self.window_size as usize + 1
    }
}

pub fn make_promoter_map<I: Iterator<Item = (String, u64, bool)>>(iter: I, half_window_size: u64) -> GIntervalMap<bool> {
    iter
        .map( |(chr, tss, is_fwd)| {
            let b = GenomicRange::new(chr, tss.saturating_sub(half_window_size), tss + half_window_size + 1);
            (b, is_fwd)
        }).collect()
}

/// barcode counting.
pub fn get_barcode_count<I>(fragments: I) -> HashMap<String, u64>
where
    I: Iterator<Item = Fragment>,
{
    let mut barcodes = HashMap::new();
    fragments.for_each(|frag| {
        let key = frag.barcode.unwrap().clone();
        *barcodes.entry(key).or_insert(0) += 1;
    });
    barcodes
}

pub struct TSSe<'a> {
    promoters: &'a TssRegions,
    counts: Vec<u64>,
    n_overlapping: u64,
    n_total: u64,
}

impl<'a> TSSe<'a> {
    pub fn new(promoters: &'a TssRegions) -> Self {
        Self { counts: vec![0; promoters.len()], n_overlapping: 0, n_total: 0, promoters }
    }

    pub fn get_counts(&self) -> &[u64] {
        &self.counts
    }

    pub fn add(&mut self, frag: &Fragment) {
        frag.to_insertions().into_iter().for_each(|ins| {
            self.n_total += 1;
            let mut overlapped = false;
            self.promoters.promoters.find(&ins).for_each(|(promoter, is_fwd)| {
                overlapped = true;
                let pos = if *is_fwd {
                        ins.start() - promoter.start()
                    } else {
                        promoter.end() - 1 - ins.start()
                    };
                self.counts[pos as usize] += 1;
            });
            if overlapped {
                self.n_overlapping += 1;
            }
        });
    }

    pub fn add_from(&mut self, tsse: &TSSe) {
        self.n_overlapping += tsse.n_overlapping;
        self.n_total += tsse.n_total;
        self.counts.iter_mut().zip(tsse.counts.iter()).for_each(|(a, b)| *a += b);
    }

    pub fn result(&self) -> (f64, f64) {
        let counts = &self.counts;
        let left_end_sum = counts.iter().take(100).sum::<u64>();
        let right_end_sum = counts.iter().rev().take(100).sum::<u64>();
        let background: f64 = (left_end_sum + right_end_sum) as f64 / 200.0 + 0.1;
        let tss_count = moving_average(5, &counts).nth(self.promoters.window_size as usize).unwrap();
        (tss_count / background, self.n_overlapping as f64 / self.n_total as f64)
    }
}

/// Compute the fragment size distribution.
/// The result is stored in a vector where each element represents the number of fragments
/// and the index represents the fragment length. The first posision of the vector is
/// reserved for fragments with length larger than the maximum length.
pub fn fragment_size_distribution<I>(data: I, max_size: usize) -> Vec<usize>
  where
    I: Iterator<Item = CsrNonCanonical<u32>>,
{
    let mut size_dist = vec![0; max_size+1];
    data.for_each(|csr| {
        let values = csr.values();
        values.iter().for_each(|&v| {
            let v = v as usize;
            if v <= max_size {
                size_dist[v] += 1;
            } else {
                size_dist[0] += 1;
            }
        });
    });
    size_dist
}

/// Count the fraction of the reads or read pairs in the given regions.
pub fn fraction_of_reads_in_region<'a, I, D>(
    iter: I, regions: &'a Vec<GIntervalMap<D>>, normalized: bool, count_as_insertion: bool,
) -> impl Iterator<Item = (Vec<Vec<f64>>, usize, usize)> + 'a
where
    I: Iterator<Item = (Vec<Vec<Fragment>>, usize, usize)> + 'a,
{
    let k = regions.len();
    iter.map(move |(data, start, end)| {
        let frac = data.into_iter().map(|fragments| {
            let mut sum = 0.0;
            let mut counts = vec![0.0; k];

            if count_as_insertion {
                fragments.into_iter().flat_map(|x| x.to_insertions()).for_each(|ins| {
                    sum += 1.0;
                    regions.iter().enumerate().for_each(|(i, r)|
                        if r.is_overlapped(&ins) {
                            counts[i] += 1.0;
                        }
                    )
                });
            } else {
                fragments.into_iter().for_each(|read| {
                    sum += 1.0;
                    regions.iter().enumerate().for_each(|(i, r)|
                        if r.is_overlapped(&read) {
                            counts[i] += 1.0;
                        }
                    )
                });
            }

            if normalized {
                counts.iter_mut().for_each(|x| *x /= sum);
            }
            counts
        }).collect::<Vec<_>>();
        (frac, start, end)
    })
}
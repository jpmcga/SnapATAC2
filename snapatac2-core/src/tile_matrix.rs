use crate::utils::anndata::{AnnDataIO, StrVec};
use crate::qc::{read_insertions};
use crate::peak_matrix::create_feat_matrix;

use anndata_rs::base::AnnData;
use anndata_rs::anndata_trait::WriteData;
use anndata_rs::iterator::CsrIterator;
use polars::prelude::{DataFrame, Series};

use hdf5::{File, Result};
use bed_utils::bed::{
    GenomicRange,
    tree::{SparseBinnedCoverage},
};

/// Create cell by bin matrix, and compute qc matrix.
/// 
/// # Arguments
/// 
/// * `file` - 
/// * `fragments` -
/// * `promoter` -
/// * `region` -
/// * `bin_size` -
/// * `min_num_fragment` -
/// * `min_tsse` -
pub fn create_tile_matrix(
    anndata: &mut AnnData,
    bin_size: u64,
    ) -> Result<()>
where
{
    let df: DataFrame = anndata.uns.get("reference_sequences").unwrap().0
        .as_ref().as_ref().read_elem();
    let regions = df.column("reference_seq_length")
        .unwrap().u64().unwrap().into_iter()
        .zip(df.column("reference_seq_name").unwrap().utf8().unwrap())
        .map(|(s, chr)| GenomicRange::new(chr.unwrap(), 0, s.unwrap())).collect();
    let feature_counter: SparseBinnedCoverage<'_, _, u32> =
        SparseBinnedCoverage::new(&regions, bin_size);
    let insertion = read_insertions(anndata)?;
    create_feat_matrix(anndata, insertion.iter(), feature_counter)
}
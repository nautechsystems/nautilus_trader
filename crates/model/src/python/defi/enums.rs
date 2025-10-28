// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Python bindings for DeFi enums.

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{PyTypeInfo, prelude::*, types::PyType};

use crate::{
    defi::{chain::Blockchain, data::PoolLiquidityUpdateType, dex::AmmType},
    python::common::EnumIterator,
};

#[pymethods]
impl Blockchain {
    #[new]
    fn py_new(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(&t, value)
    }

    fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(Blockchain),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> u8 {
        *self as u8
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "ABSTRACT")]
    fn py_abstract() -> Self {
        Self::Abstract
    }

    #[classattr]
    #[pyo3(name = "ARBITRUM")]
    fn py_arbitrum() -> Self {
        Self::Arbitrum
    }

    #[classattr]
    #[pyo3(name = "ARBITRUM_NOVA")]
    fn py_arbitrum_nova() -> Self {
        Self::ArbitrumNova
    }

    #[classattr]
    #[pyo3(name = "ARBITRUM_SEPOLIA")]
    fn py_arbitrum_sepolia() -> Self {
        Self::ArbitrumSepolia
    }

    #[classattr]
    #[pyo3(name = "AURORA")]
    fn py_aurora() -> Self {
        Self::Aurora
    }

    #[classattr]
    #[pyo3(name = "AVALANCHE")]
    fn py_avalanche() -> Self {
        Self::Avalanche
    }

    #[classattr]
    #[pyo3(name = "BASE")]
    fn py_base() -> Self {
        Self::Base
    }

    #[classattr]
    #[pyo3(name = "BASE_SEPOLIA")]
    fn py_base_sepolia() -> Self {
        Self::BaseSepolia
    }

    #[classattr]
    #[pyo3(name = "BERACHAIN")]
    fn py_berachain() -> Self {
        Self::Berachain
    }

    #[classattr]
    #[pyo3(name = "BERACHAIN_BARTIO")]
    fn py_berachain_bartio() -> Self {
        Self::BerachainBartio
    }

    #[classattr]
    #[pyo3(name = "BLAST")]
    fn py_blast() -> Self {
        Self::Blast
    }

    #[classattr]
    #[pyo3(name = "BLAST_SEPOLIA")]
    fn py_blast_sepolia() -> Self {
        Self::BlastSepolia
    }

    #[classattr]
    #[pyo3(name = "BOBA")]
    fn py_boba() -> Self {
        Self::Boba
    }

    #[classattr]
    #[pyo3(name = "BSC")]
    fn py_bsc() -> Self {
        Self::Bsc
    }

    #[classattr]
    #[pyo3(name = "BSC_TESTNET")]
    fn py_bsc_testnet() -> Self {
        Self::BscTestnet
    }

    #[classattr]
    #[pyo3(name = "CELO")]
    fn py_celo() -> Self {
        Self::Celo
    }

    #[classattr]
    #[pyo3(name = "CHILIZ")]
    fn py_chiliz() -> Self {
        Self::Chiliz
    }

    #[classattr]
    #[pyo3(name = "CITREA_TESTNET")]
    fn py_citrea_testnet() -> Self {
        Self::CitreaTestnet
    }

    #[classattr]
    #[pyo3(name = "CURTIS")]
    fn py_curtis() -> Self {
        Self::Curtis
    }

    #[classattr]
    #[pyo3(name = "CYBER")]
    fn py_cyber() -> Self {
        Self::Cyber
    }

    #[classattr]
    #[pyo3(name = "DARWINIA")]
    fn py_darwinia() -> Self {
        Self::Darwinia
    }

    #[classattr]
    #[pyo3(name = "ETHEREUM")]
    fn py_ethereum() -> Self {
        Self::Ethereum
    }

    #[classattr]
    #[pyo3(name = "FANTOM")]
    fn py_fantom() -> Self {
        Self::Fantom
    }

    #[classattr]
    #[pyo3(name = "FLARE")]
    fn py_flare() -> Self {
        Self::Flare
    }

    #[classattr]
    #[pyo3(name = "FRAXTAL")]
    fn py_fraxtal() -> Self {
        Self::Fraxtal
    }

    #[classattr]
    #[pyo3(name = "FUJI")]
    fn py_fuji() -> Self {
        Self::Fuji
    }

    #[classattr]
    #[pyo3(name = "GALADRIEL_DEVNET")]
    fn py_galadriel_devnet() -> Self {
        Self::GaladrielDevnet
    }

    #[classattr]
    #[pyo3(name = "GNOSIS")]
    fn py_gnosis() -> Self {
        Self::Gnosis
    }

    #[classattr]
    #[pyo3(name = "GNOSIS_CHIADO")]
    fn py_gnosis_chiado() -> Self {
        Self::GnosisChiado
    }

    #[classattr]
    #[pyo3(name = "GNOSIS_TRACES")]
    fn py_gnosis_traces() -> Self {
        Self::GnosisTraces
    }

    #[classattr]
    #[pyo3(name = "HARMONY_SHARD_0")]
    fn py_harmony_shard_0() -> Self {
        Self::HarmonyShard0
    }

    #[classattr]
    #[pyo3(name = "HOLESKY")]
    fn py_holesky() -> Self {
        Self::Holesky
    }

    #[classattr]
    #[pyo3(name = "HOLESKY_TOKEN_TEST")]
    fn py_holesky_token_test() -> Self {
        Self::HoleskyTokenTest
    }

    #[classattr]
    #[pyo3(name = "HYPERLIQUID")]
    fn py_hyperliquid() -> Self {
        Self::Hyperliquid
    }

    #[classattr]
    #[pyo3(name = "HYPERLIQUID_TEMP")]
    fn py_hyperliquid_temp() -> Self {
        Self::HyperliquidTemp
    }

    #[classattr]
    #[pyo3(name = "INK")]
    fn py_ink() -> Self {
        Self::Ink
    }

    #[classattr]
    #[pyo3(name = "INTERNAL_TEST_CHAIN")]
    fn py_internal_test_chain() -> Self {
        Self::InternalTestChain
    }

    #[classattr]
    #[pyo3(name = "KROMA")]
    fn py_kroma() -> Self {
        Self::Kroma
    }

    #[classattr]
    #[pyo3(name = "LINEA")]
    fn py_linea() -> Self {
        Self::Linea
    }

    #[classattr]
    #[pyo3(name = "LISK")]
    fn py_lisk() -> Self {
        Self::Lisk
    }

    #[classattr]
    #[pyo3(name = "LUKSO")]
    fn py_lukso() -> Self {
        Self::Lukso
    }

    #[classattr]
    #[pyo3(name = "LUKSO_TESTNET")]
    fn py_lukso_testnet() -> Self {
        Self::LuksoTestnet
    }

    #[classattr]
    #[pyo3(name = "MANTA")]
    fn py_manta() -> Self {
        Self::Manta
    }

    #[classattr]
    #[pyo3(name = "MANTLE")]
    fn py_mantle() -> Self {
        Self::Mantle
    }

    #[classattr]
    #[pyo3(name = "MEGAETH_TESTNET")]
    fn py_megaeth_testnet() -> Self {
        Self::MegaethTestnet
    }

    #[classattr]
    #[pyo3(name = "MERLIN")]
    fn py_merlin() -> Self {
        Self::Merlin
    }

    #[classattr]
    #[pyo3(name = "METALL2")]
    fn py_metall2() -> Self {
        Self::Metall2
    }

    #[classattr]
    #[pyo3(name = "METIS")]
    fn py_metis() -> Self {
        Self::Metis
    }

    #[classattr]
    #[pyo3(name = "MEV_COMMIT")]
    fn py_mev_commit() -> Self {
        Self::MevCommit
    }

    #[classattr]
    #[pyo3(name = "MODE")]
    fn py_mode() -> Self {
        Self::Mode
    }

    #[classattr]
    #[pyo3(name = "MONAD_TESTNET")]
    fn py_monad_testnet() -> Self {
        Self::MonadTestnet
    }

    #[classattr]
    #[pyo3(name = "MONAD_TESTNET_BACKUP")]
    fn py_monad_testnet_backup() -> Self {
        Self::MonadTestnetBackup
    }

    #[classattr]
    #[pyo3(name = "MOONBASE_ALPHA")]
    fn py_moonbase_alpha() -> Self {
        Self::MoonbaseAlpha
    }

    #[classattr]
    #[pyo3(name = "MOONBEAM")]
    fn py_moonbeam() -> Self {
        Self::Moonbeam
    }

    #[classattr]
    #[pyo3(name = "MORPH")]
    fn py_morph() -> Self {
        Self::Morph
    }

    #[classattr]
    #[pyo3(name = "MORPH_HOLESKY")]
    fn py_morph_holesky() -> Self {
        Self::MorphHolesky
    }

    #[classattr]
    #[pyo3(name = "OPBNB")]
    fn py_opbnb() -> Self {
        Self::Opbnb
    }

    #[classattr]
    #[pyo3(name = "OPTIMISM")]
    fn py_optimism() -> Self {
        Self::Optimism
    }

    #[classattr]
    #[pyo3(name = "OPTIMISM_SEPOLIA")]
    fn py_optimism_sepolia() -> Self {
        Self::OptimismSepolia
    }

    #[classattr]
    #[pyo3(name = "PHAROS_DEVNET")]
    fn py_pharos_devnet() -> Self {
        Self::PharosDevnet
    }

    #[classattr]
    #[pyo3(name = "POLYGON")]
    fn py_polygon() -> Self {
        Self::Polygon
    }

    #[classattr]
    #[pyo3(name = "POLYGON_AMOY")]
    fn py_polygon_amoy() -> Self {
        Self::PolygonAmoy
    }

    #[classattr]
    #[pyo3(name = "POLYGON_ZKEVM")]
    fn py_polygon_zkevm() -> Self {
        Self::PolygonZkEvm
    }

    #[classattr]
    #[pyo3(name = "ROOTSTOCK")]
    fn py_rootstock() -> Self {
        Self::Rootstock
    }

    #[classattr]
    #[pyo3(name = "SAAKURU")]
    fn py_saakuru() -> Self {
        Self::Saakuru
    }

    #[classattr]
    #[pyo3(name = "SCROLL")]
    fn py_scroll() -> Self {
        Self::Scroll
    }

    #[classattr]
    #[pyo3(name = "SEPOLIA")]
    fn py_sepolia() -> Self {
        Self::Sepolia
    }

    #[classattr]
    #[pyo3(name = "SHIMMER_EVM")]
    fn py_shimmer_evm() -> Self {
        Self::ShimmerEvm
    }

    #[classattr]
    #[pyo3(name = "SONEIUM")]
    fn py_soneium() -> Self {
        Self::Soneium
    }

    #[classattr]
    #[pyo3(name = "SOPHON")]
    fn py_sophon() -> Self {
        Self::Sophon
    }

    #[classattr]
    #[pyo3(name = "SOPHON_TESTNET")]
    fn py_sophon_testnet() -> Self {
        Self::SophonTestnet
    }

    #[classattr]
    #[pyo3(name = "SUPERSEED")]
    fn py_supersede() -> Self {
        Self::Superseed
    }

    #[classattr]
    #[pyo3(name = "UNICHAIN")]
    fn py_unichain() -> Self {
        Self::Unichain
    }

    #[classattr]
    #[pyo3(name = "UNICHAIN_SEPOLIA")]
    fn py_unichain_sepolia() -> Self {
        Self::UnichainSepolia
    }

    #[classattr]
    #[pyo3(name = "XDC")]
    fn py_xdc() -> Self {
        Self::Xdc
    }

    #[classattr]
    #[pyo3(name = "XDC_TESTNET")]
    fn py_xdc_testnet() -> Self {
        Self::XdcTestnet
    }

    #[classattr]
    #[pyo3(name = "ZETA")]
    fn py_zeta() -> Self {
        Self::Zeta
    }

    #[classattr]
    #[pyo3(name = "ZIRCUIT")]
    fn py_zircuit() -> Self {
        Self::Zircuit
    }

    #[classattr]
    #[pyo3(name = "ZKSYNC")]
    fn py_zksync() -> Self {
        Self::ZKsync
    }

    #[classattr]
    #[pyo3(name = "ZORA")]
    fn py_zora() -> Self {
        Self::Zora
    }
}

#[pymethods]
impl AmmType {
    #[new]
    fn py_new(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(&t, value)
    }

    fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(AmmType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> u8 {
        *self as u8
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        Self::from_str(data_str).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "CPAMM")]
    fn py_cpamm() -> Self {
        Self::CPAMM
    }

    #[classattr]
    #[pyo3(name = "CLAMM")]
    fn py_clamm() -> Self {
        Self::CLAMM
    }

    #[classattr]
    #[pyo3(name = "CLAM_ENHANCED")]
    fn py_clam_enhanced() -> Self {
        Self::CLAMEnhanced
    }

    #[classattr]
    #[pyo3(name = "STABLE_SWAP")]
    fn py_stable_swap() -> Self {
        Self::StableSwap
    }

    #[classattr]
    #[pyo3(name = "WEIGHTED_POOL")]
    fn py_weighted_pool() -> Self {
        Self::WeightedPool
    }

    #[classattr]
    #[pyo3(name = "COMPOSABLE_POOL")]
    fn py_composable_pool() -> Self {
        Self::ComposablePool
    }
}

#[pymethods]
impl PoolLiquidityUpdateType {
    #[new]
    fn py_new(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let t = Self::type_object(py);
        Self::py_from_str(&t, value)
    }

    fn __hash__(&self) -> isize {
        *self as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(PoolLiquidityUpdateType),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> u8 {
        *self as u8
    }

    #[classmethod]
    fn variants(_: &Bound<'_, PyType>, py: Python<'_>) -> EnumIterator {
        EnumIterator::new::<Self>(py)
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        Self::from_str(data_str).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "MINT")]
    fn py_mint() -> Self {
        Self::Mint
    }

    #[classattr]
    #[pyo3(name = "BURN")]
    fn py_burn() -> Self {
        Self::Burn
    }
}

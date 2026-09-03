//! Feature-gated Prometheus re-export for daemon metrics.
//!
//! Same contract as `xai-grok-workspace::prometheus_facade`. This crate cannot
//! depend on workspace (workspace depends on the daemon), so the facade is
//! duplicated here. When `prometheus-metrics` is off, call sites keep the same
//! API against no-op types and the prometheus crate is absent from slim.

#[cfg(feature = "prometheus-metrics")]
mod imp {
    pub use prometheus::{
        Histogram, HistogramVec, IntCounter, IntCounterVec, IntGauge, register_histogram,
        register_histogram_vec, register_int_counter, register_int_counter_vec, register_int_gauge,
    };

    pub fn gather() -> Vec<prometheus::proto::MetricFamily> {
        prometheus::gather()
    }
}

#[cfg(not(feature = "prometheus-metrics"))]
mod imp {
    #[derive(Clone, Copy, Debug, Default)]
    pub struct IntCounter;
    impl IntCounter {
        pub fn inc(&self) {}
        pub fn inc_by(&self, _: u64) {}
        pub fn get(&self) -> u64 {
            0
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct IntGauge;
    impl IntGauge {
        pub fn inc(&self) {}
        pub fn dec(&self) {}
        pub fn set(&self, _: i64) {}
        pub fn get(&self) -> i64 {
            0
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Histogram;
    impl Histogram {
        pub fn observe(&self, _: f64) {}
        pub fn get_sample_count(&self) -> u64 {
            0
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct IntCounterVec;
    impl IntCounterVec {
        pub fn with_label_values(&self, _: &[&str]) -> &IntCounter {
            static C: IntCounter = IntCounter;
            &C
        }
    }

    #[derive(Clone, Copy, Debug, Default)]
    pub struct HistogramVec;
    impl HistogramVec {
        pub fn with_label_values(&self, _: &[&str]) -> &Histogram {
            static H: Histogram = Histogram;
            &H
        }
    }

    pub fn gather() -> Vec<()> {
        Vec::new()
    }

    #[macro_export]
    macro_rules! register_int_counter {
        ($name:expr, $help:expr $(,)?) => {
            ::core::result::Result::<
                $crate::prometheus_facade::IntCounter,
                &'static str,
            >::Ok($crate::prometheus_facade::IntCounter)
        };
    }

    #[macro_export]
    macro_rules! register_int_counter_vec {
        ($name:expr, $help:expr, $labels:expr $(,)?) => {
            ::core::result::Result::<
                $crate::prometheus_facade::IntCounterVec,
                &'static str,
            >::Ok($crate::prometheus_facade::IntCounterVec)
        };
    }

    #[macro_export]
    macro_rules! register_int_gauge {
        ($name:expr, $help:expr $(,)?) => {
            ::core::result::Result::<
                $crate::prometheus_facade::IntGauge,
                &'static str,
            >::Ok($crate::prometheus_facade::IntGauge)
        };
    }

    #[macro_export]
    macro_rules! register_histogram {
        ($name:expr, $help:expr $(,)?) => {
            ::core::result::Result::<
                $crate::prometheus_facade::Histogram,
                &'static str,
            >::Ok($crate::prometheus_facade::Histogram)
        };
        ($name:expr, $help:expr, $buckets:expr $(,)?) => {
            ::core::result::Result::<
                $crate::prometheus_facade::Histogram,
                &'static str,
            >::Ok($crate::prometheus_facade::Histogram)
        };
    }

    #[macro_export]
    macro_rules! register_histogram_vec {
        ($name:expr, $help:expr, $labels:expr $(,)?) => {
            ::core::result::Result::<
                $crate::prometheus_facade::HistogramVec,
                &'static str,
            >::Ok($crate::prometheus_facade::HistogramVec)
        };
        ($name:expr, $help:expr, $labels:expr, $buckets:expr $(,)?) => {
            ::core::result::Result::<
                $crate::prometheus_facade::HistogramVec,
                &'static str,
            >::Ok($crate::prometheus_facade::HistogramVec)
        };
    }

    pub use crate::{
        register_histogram, register_histogram_vec, register_int_counter, register_int_counter_vec,
        register_int_gauge,
    };
}

pub use imp::*;

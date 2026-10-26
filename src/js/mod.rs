// SPDX-License-Identifier: MIT
// Copyright (c) 2018+, MarkedJS (https://github.com/markedjs/)
// Copyright (c) 2011-2018, Christopher Jeffrey (marked's original author)

//! The parts of the JavaScript runtime the library relies on: UTF-16 strings and their methods, the Unicode data
//! behind them, values, number printing, `JSON` and thrown errors.

pub mod error;
pub mod json;
pub mod number;
pub mod string;
pub mod unicode_data;
pub mod value;

pub use error::JsError;
pub use string::JsString;
pub use value::{Object, Value};

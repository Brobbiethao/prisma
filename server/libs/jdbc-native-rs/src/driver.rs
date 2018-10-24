#![allow(unused, unused_mut)]

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use chrono::prelude::*;
use chrono::format;
use rust_decimal::Decimal;
use serde_json;
use uuid;

use postgres;
use postgres::rows::{Row, Rows};
use postgres::stmt::Statement;
use postgres::transaction::Transaction;
use postgres::types::{IsNull, ToSql, Type};
use postgres::{Connection, Result as PsqlResult, TlsMode};

use std::boxed::Box;
use std::cell;
use std::cell::RefCell;
use std::error::Error as StdErr;
use std::result;

use num_traits::ToPrimitive;
use num_traits::cast::FromPrimitive;

use jdbc_params;

#[repr(C)]
#[no_mangle]
#[allow(non_snake_case)]
pub struct PsqlConnection<'a> {
    connection: Connection,
    transaction: RefCell<Option<Transaction<'a>>>,
}

#[repr(C)]
#[no_mangle]
#[allow(non_snake_case)]
pub struct PsqlPreparedStatement<'a> {
    statement: Statement<'a>,
}

impl<'a> PsqlPreparedStatement<'a> {
    pub fn execute(&self, params: Vec<Vec<&jdbc_params::JdbcParameter>>) -> Result<Vec<i32>> {
        let paramLength = params.len();
        let mut counts = Vec::new();

        for param in params {
            let res = self.statement.execute(&jdbc_params::JdbcParameter::paramsToSql(param)[..]).map_err(DriverError::from);
            match res {
                Ok(count) => counts.push(count as i32),
                Err(ref e) if paramLength > 1 => {
                    println!("[Rust] Error during prep exec: {:?}", e);
                    counts.push(-3)
                },
                Err(_) => return res.map(|v| vec!(v as i32))
            }
        }

        Ok(counts)
    }

    pub fn query(&self, params: Vec<&jdbc_params::JdbcParameter>) -> Result<Rows> {
        self.statement.query(&jdbc_params::JdbcParameter::paramsToSql(params)[..]).map_err(DriverError::from)
    }
}

impl<'a> Drop for PsqlPreparedStatement<'a> {
    fn drop(&mut self) {
        println!("[Rust] Dropping prepared statement");
    }
}


#[derive(Debug)]
pub enum DriverError {
    JsonError(serde_json::Error),
    PsqlError(postgres::Error),
    GenericError(String),
}

pub type Result<T> = result::Result<T, DriverError>;

pub fn connect<'a>(url: String) -> PsqlConnection<'a> {
    let conn = Connection::connect(url, TlsMode::None).unwrap();
    return PsqlConnection {
        connection: conn,
        transaction: RefCell::new(None),
    };
}

impl<'a> Drop for PsqlConnection<'a> {
    fn drop(&mut self) {
        println!("[Rust] Dropping psql connection");
    }
}

impl From<postgres::Error> for DriverError {
    fn from(e: postgres::Error) -> Self {
        DriverError::PsqlError(e)
    }
}

impl From<serde_json::Error> for DriverError {
    fn from(e: serde_json::Error) -> Self {
        DriverError::JsonError(e)
    }
}

impl From<cell::BorrowMutError> for DriverError {
    fn from(e: cell::BorrowMutError) -> Self {
        DriverError::GenericError(e.to_string())
    }
}

impl From<format::ParseError> for DriverError {
    fn from(e: format::ParseError) -> Self {
        DriverError::GenericError(e.to_string())
    }
}

impl From<uuid::ParseError> for DriverError {
    fn from(e: uuid::ParseError) -> Self {
        DriverError::GenericError(e.to_string())
    }
}

impl<'a> PsqlConnection<'a> {
    pub fn prepareStatement(&self, query: String) -> Result<PsqlPreparedStatement> {
        let stmt = self.connection.prepare(query.as_str())?;
        Ok(PsqlPreparedStatement{ statement: stmt })
    }

    pub fn query(&self, query: String, params: Vec<&jdbc_params::JdbcParameter>) -> Result<Rows> {
        println!("[Rust] Query received the params: {:?}", params);
        let mutRef = self.transaction.try_borrow_mut()?;
        let rows = match *mutRef {
            Some(ref t) => t.query(&*query, &jdbc_params::JdbcParameter::paramsToSql(params)[..])?,
            None => self.connection.query(&*query, &jdbc_params::JdbcParameter::paramsToSql(params)[..])?,
        };

        println!("[Rust] The result set has {} columns", rows.columns().len());
        for column in rows.columns() {
            println!("[Rust] column {} of type {}", column.name(), column.type_());
        }

        return Ok(rows);
    }

    pub fn execute(&self, query: String, params: Vec<&jdbc_params::JdbcParameter>) -> Result<u64> {
        println!("[Rust] Execute received the params: {:?}", params);

        let mutRef = self.transaction.try_borrow_mut()?;
        let result = match *mutRef {
            Some(ref t) => {
                println!("[Rust] Have transaction");
                t.execute(&*query, &jdbc_params::JdbcParameter::paramsToSql(params)[..])?
            }
            None => self.connection.execute(&*query, &jdbc_params::JdbcParameter::paramsToSql(params)[..])?,
        };

        println!("[Rust] EXEC DONE");
        return Ok(result);
    }

    pub fn close(self) {
        // Simply drops the moved var for now, which calls the drop Impl
    }

    pub fn startTransaction(&'a mut self) -> Result<()> {
        let ta = self.connection.transaction()?;
        self.transaction.replace(Some(ta));

        return Ok(());
    }

    pub fn commitTransaction(&self) -> Result<()> {
        let taOpt = self.transaction.replace(None);
        match taOpt {
            Some(ta) => {
                println!("[Rust] Have transaction");
                Ok(ta.commit()?)
            }
            None => Ok(()),
        }
    }

    pub fn rollbackTransaction(&self) -> Result<()> {
        let taOpt = self.transaction.replace(None);
        match taOpt {
            Some(ta) => {
                println!("[Rust] Have transaction");
                Ok(ta.set_rollback())
            }

            None => Ok(()),
        }
    }
}

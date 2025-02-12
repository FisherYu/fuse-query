// Copyright 2020 The FuseQuery Authors.
//
// Code is licensed under AGPL License, Version 3.0.

use std::fmt;

use crate::datablocks::DataBlock;
use crate::datavalues;
use crate::datavalues::{
    DataColumnarValue, DataSchema, DataType, DataValue, DataValueAggregateOperator,
    DataValueArithmeticOperator,
};
use crate::error::{FuseQueryError, FuseQueryResult};
use crate::functions::Function;

#[derive(Clone, Debug)]
pub struct AggregatorFunction {
    depth: usize,
    op: DataValueAggregateOperator,
    arg: Box<Function>,
    state: DataValue,
}

impl AggregatorFunction {
    pub fn try_create_count_func(args: &[Function]) -> FuseQueryResult<Function> {
        Self::try_create(DataValueAggregateOperator::Count, args)
    }

    pub fn try_create_max_func(args: &[Function]) -> FuseQueryResult<Function> {
        Self::try_create(DataValueAggregateOperator::Max, args)
    }

    pub fn try_create_min_func(args: &[Function]) -> FuseQueryResult<Function> {
        Self::try_create(DataValueAggregateOperator::Min, args)
    }

    pub fn try_create_sum_func(args: &[Function]) -> FuseQueryResult<Function> {
        Self::try_create(DataValueAggregateOperator::Sum, args)
    }

    pub fn try_create_avg_func(args: &[Function]) -> FuseQueryResult<Function> {
        Self::try_create(DataValueAggregateOperator::Avg, args)
    }

    fn try_create(op: DataValueAggregateOperator, args: &[Function]) -> FuseQueryResult<Function> {
        if args.len() != 1 {
            return Err(FuseQueryError::Internal(format!(
                "Aggregator function {:?} args require single argument",
                op
            )));
        }

        let state = match op {
            DataValueAggregateOperator::Count => DataValue::UInt64(Some(0)),
            DataValueAggregateOperator::Avg => DataValue::Struct(vec![
                DataValue::Float64(Some(0.0)),
                DataValue::UInt64(Some(0)),
            ]),
            _ => DataValue::Null,
        };
        Ok(Function::Aggregator(AggregatorFunction {
            depth: 0,
            op,
            arg: Box::new(args[0].clone()),
            state,
        }))
    }

    pub fn return_type(&self, input_schema: &DataSchema) -> FuseQueryResult<DataType> {
        self.arg.return_type(input_schema)
    }

    pub fn nullable(&self, _input_schema: &DataSchema) -> FuseQueryResult<bool> {
        Ok(false)
    }

    pub fn set_depth(&mut self, depth: usize) {
        self.depth = depth;
    }

    pub fn eval(&mut self, block: &DataBlock) -> FuseQueryResult<DataColumnarValue> {
        self.arg.eval(block)
    }

    pub fn accumulate(&mut self, block: &DataBlock) -> FuseQueryResult<()> {
        let rows = block.num_rows();
        let val = self.arg.eval(&block)?;
        match &self.op {
            DataValueAggregateOperator::Count => {
                self.state = datavalues::data_value_arithmetic_op(
                    DataValueArithmeticOperator::Add,
                    self.state.clone(),
                    DataValue::UInt64(Some(rows as u64)),
                )?;
            }
            DataValueAggregateOperator::Min => {
                self.state = datavalues::data_value_aggregate_op(
                    DataValueAggregateOperator::Min,
                    self.state.clone(),
                    datavalues::data_array_aggregate_op(
                        DataValueAggregateOperator::Min,
                        val.to_array(rows)?,
                    )?,
                )?;
            }
            DataValueAggregateOperator::Max => {
                self.state = datavalues::data_value_aggregate_op(
                    DataValueAggregateOperator::Max,
                    self.state.clone(),
                    datavalues::data_array_aggregate_op(
                        DataValueAggregateOperator::Max,
                        val.to_array(rows)?,
                    )?,
                )?;
            }
            DataValueAggregateOperator::Sum => {
                self.state = datavalues::data_value_arithmetic_op(
                    DataValueArithmeticOperator::Add,
                    self.state.clone(),
                    datavalues::data_array_aggregate_op(
                        DataValueAggregateOperator::Sum,
                        val.to_array(rows)?,
                    )?,
                )?;
            }
            DataValueAggregateOperator::Avg => {
                if let DataValue::Struct(values) = self.state.clone() {
                    let sum = datavalues::data_value_arithmetic_op(
                        DataValueArithmeticOperator::Add,
                        values[0].clone(),
                        datavalues::data_array_aggregate_op(
                            DataValueAggregateOperator::Sum,
                            val.to_array(1)?,
                        )?,
                    )?;
                    let count = datavalues::data_value_arithmetic_op(
                        DataValueArithmeticOperator::Add,
                        values[1].clone(),
                        DataValue::UInt64(Some(rows as u64)),
                    )?;

                    self.state = DataValue::Struct(vec![sum, count]);
                }
            }
        }
        Ok(())
    }

    pub fn accumulate_result(&self) -> FuseQueryResult<Vec<DataValue>> {
        Ok(vec![self.state.clone()])
    }

    pub fn merge(&mut self, states: &[DataValue]) -> FuseQueryResult<()> {
        let val = states[self.depth].clone();
        match &self.op {
            DataValueAggregateOperator::Count => {
                self.state = datavalues::data_value_arithmetic_op(
                    DataValueArithmeticOperator::Add,
                    self.state.clone(),
                    val,
                )?;
            }
            DataValueAggregateOperator::Min => {
                self.state = datavalues::data_value_aggregate_op(
                    DataValueAggregateOperator::Min,
                    self.state.clone(),
                    val,
                )?;
            }
            DataValueAggregateOperator::Max => {
                self.state = datavalues::data_value_aggregate_op(
                    DataValueAggregateOperator::Max,
                    self.state.clone(),
                    val,
                )?;
            }
            DataValueAggregateOperator::Sum => {
                self.state = datavalues::data_value_arithmetic_op(
                    DataValueArithmeticOperator::Add,
                    self.state.clone(),
                    val,
                )?;
            }
            DataValueAggregateOperator::Avg => {
                if let (DataValue::Struct(new_states), DataValue::Struct(old_states)) =
                    (val, self.state.clone())
                {
                    let sum = datavalues::data_value_arithmetic_op(
                        DataValueArithmeticOperator::Add,
                        new_states[0].clone(),
                        old_states[0].clone(),
                    )?;
                    let count = datavalues::data_value_arithmetic_op(
                        DataValueArithmeticOperator::Add,
                        new_states[1].clone(),
                        old_states[1].clone(),
                    )?;
                    self.state = DataValue::Struct(vec![sum, count]);
                }
            }
        }
        Ok(())
    }

    pub fn merge_result(&self) -> FuseQueryResult<DataValue> {
        Ok(match self.op {
            DataValueAggregateOperator::Avg => {
                if let DataValue::Struct(states) = self.state.clone() {
                    datavalues::data_value_arithmetic_op(
                        DataValueArithmeticOperator::Div,
                        states[0].clone(),
                        states[1].clone(),
                    )?
                } else {
                    self.state.clone()
                }
            }
            _ => self.state.clone(),
        })
    }
}

impl fmt::Display for AggregatorFunction {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}({:?})", self.op, self.arg)
    }
}

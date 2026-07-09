use super::{TranslationPreference, ValueKind, check_single_wordlength};
use crate::wave_container::{ScopeId, VarId, VariableMeta};

use eyre::Result;
use instruction_decoder::{Decoder, specs};
use surfer_translation_types::{BasicTranslator, VariableValue, check_vector_variable};

pub struct InstructionTranslator {
    pub name: String,
    pub decoder: Decoder,
    pub num_bits: u32,
}

impl BasicTranslator<VarId, ScopeId> for InstructionTranslator {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn basic_translate(&self, num_bits: u32, value: &VariableValue) -> (String, ValueKind) {
        let u64_value = match value {
            VariableValue::BigUint(v) => v.to_u64_digits().last().copied(),
            VariableValue::String(s) => match check_vector_variable(s) {
                Some(v) => return v,
                None => u64::from_str_radix(s, 2).ok(),
            },
        }
        .unwrap_or(0);

        match self
            .decoder
            .decode_from_i64(u64_value as i64, num_bits as usize)
        {
            Ok(iform) => (iform, ValueKind::Normal),
            _ => (
                format!(
                    "UNKNOWN INSN ({:#0width$x})",
                    u64_value,
                    width = num_bits.div_ceil(4) as usize + 2
                ),
                ValueKind::Warn,
            ),
        }
    }

    fn translates(&self, variable: &VariableMeta) -> Result<TranslationPreference> {
        check_single_wordlength(variable.num_bits, self.num_bits)
    }
}

#[must_use]
pub fn new_rv32_translator() -> InstructionTranslator {
    InstructionTranslator {
        name: "RV32".into(),
        decoder: Decoder::new(
            &specs::rv32::RV32
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<String>>(),
        )
        .expect("Can't build RV32 decoder"),
        num_bits: 32,
    }
}

#[must_use]
pub fn new_rv64_translator() -> InstructionTranslator {
    InstructionTranslator {
        name: "RV64".into(),
        decoder: Decoder::new(
            &specs::rv64::RV64
                .iter()
                .map(std::string::ToString::to_string)
                .collect::<Vec<String>>(),
        )
        .expect("Can't build RV64 decoder"),
        num_bits: 32,
    }
}

#[must_use]
pub fn new_mips_translator() -> InstructionTranslator {
    InstructionTranslator {
        name: "MIPS".into(),
        decoder: Decoder::new(&[specs::MIPS.to_owned()]).expect("Can't build mips decoder"),
        num_bits: 32,
    }
}

#[must_use]
pub fn new_la64_translator() -> InstructionTranslator {
    InstructionTranslator {
        name: "LA64".into(),
        decoder: Decoder::new(&[specs::LA64.to_owned()]).expect("Can't build LA64 decoder"),
        num_bits: 32,
    }
}

#[cfg(test)]
mod test {

    use super::*;

    #[test]
    fn riscv_from_bigunit() {
        let rv32_translator = new_rv32_translator();
        let rv64_translator = new_rv64_translator();
        assert_eq!(
            rv32_translator
                .basic_translate(32, &VariableValue::BigUint(1u32.into()))
                .0,
            "c.nop"
        );
        assert_eq!(
            rv32_translator
                .basic_translate(32, &VariableValue::BigUint(0b1000000010011111u32.into()))
                .0,
            "UNKNOWN INSN (0x0000809f)"
        );
        assert_eq!(
            rv32_translator
                .basic_translate(
                    32,
                    &VariableValue::BigUint(0b1000_0001_0011_0101_0000_0101_1001_0011_u32.into())
                )
                .0,
            "addi a1, a0, -2029"
        );
        assert_eq!(
            rv64_translator
                .basic_translate(32, &VariableValue::BigUint(1u32.into()))
                .0,
            "c.nop"
        );
        assert_eq!(
            rv64_translator
                .basic_translate(32, &VariableValue::BigUint(0b1000000010011111u32.into()))
                .0,
            "UNKNOWN INSN (0x0000809f)"
        );
        assert_eq!(
            rv64_translator
                .basic_translate(
                    32,
                    &VariableValue::BigUint(0b1000_0001_0011_0101_0000_0101_1001_0011_u32.into())
                )
                .0,
            "addi a1, a0, -2029"
        );
    }
    #[test]
    fn riscv_from_string() {
        let rv32_translator = new_rv32_translator();
        assert_eq!(
            rv32_translator
                .basic_translate(32, &VariableValue::String("1".to_owned()))
                .0,
            "c.nop"
        );
        assert_eq!(
            rv32_translator
                .basic_translate(
                    32,
                    &VariableValue::String("01001000100010001000100011111111".to_owned())
                )
                .0,
            "UNKNOWN INSN (0x488888ff)"
        );
        assert_eq!(
            rv32_translator
                .basic_translate(
                    32,
                    &VariableValue::String("01xzz-hlw0010001000100010001000".to_owned())
                )
                .0,
            "UNDEF"
        );
        assert_eq!(
            rv32_translator
                .basic_translate(
                    32,
                    &VariableValue::String("010zz-hlw0010001000100010001000".to_owned())
                )
                .0,
            "HIGHIMP"
        );
        assert_eq!(
            rv32_translator
                .basic_translate(
                    32,
                    &VariableValue::String("01011-hlw0010001000100010001000".to_owned())
                )
                .0,
            "DON'T CARE"
        );
    }

    #[test]
    fn mips_from_bigunit() {
        let mips_translator = new_mips_translator();
        assert_eq!(
            mips_translator
                .basic_translate(32, &VariableValue::BigUint(0x3a873u32.into()))
                .0,
            "UNKNOWN INSN (0x0003a873)"
        );
        assert_eq!(
            mips_translator
                .basic_translate(32, &VariableValue::BigUint(0x24210000u32.into()))
                .0,
            "addiu $at, $at, 0"
        );
    }

    #[test]
    fn mips_from_string() {
        let mips_translator = new_mips_translator();
        assert_eq!(
            mips_translator
                .basic_translate(
                    32,
                    &VariableValue::String("10101111110000010000000000000000".to_owned())
                )
                .0,
            "sw $at, 0($fp)"
        );
        assert_eq!(
            mips_translator
                .basic_translate(
                    32,
                    &VariableValue::String("01xzz-hlw0010001000100010001000".to_owned())
                )
                .0,
            "UNDEF"
        );
        assert_eq!(
            mips_translator
                .basic_translate(
                    32,
                    &VariableValue::String("010zz-hlw0010001000100010001000".to_owned())
                )
                .0,
            "HIGHIMP"
        );
        assert_eq!(
            mips_translator
                .basic_translate(
                    32,
                    &VariableValue::String("01011-hlw0010001000100010001000".to_owned())
                )
                .0,
            "DON'T CARE"
        );
    }

    #[test]
    fn la64_from_bigunit() {
        let la64_translator = new_la64_translator();
        assert_eq!(
            la64_translator
                .basic_translate(32, &VariableValue::BigUint(0xffffffffu32.into()))
                .0,
            "UNKNOWN INSN (0xffffffff)"
        );
        assert_eq!(
            la64_translator
                .basic_translate(32, &VariableValue::BigUint(0x1a000004u32.into()))
                .0,
            "pcalau12i $a0, 0"
        );
    }

    #[test]
    fn la64_from_string() {
        let la64_translator = new_la64_translator();
        assert_eq!(
            la64_translator
                .basic_translate(
                    32,
                    &VariableValue::String("00101001101111111011001011001100".to_owned())
                )
                .0,
            "st.w $t0, $fp, -20"
        );
        assert_eq!(
            la64_translator
                .basic_translate(
                    32,
                    &VariableValue::String("01xzz-hlw0010001000100010001000".to_owned())
                )
                .0,
            "UNDEF"
        );
        assert_eq!(
            la64_translator
                .basic_translate(
                    32,
                    &VariableValue::String("010zz-hlw0010001000100010001000".to_owned())
                )
                .0,
            "HIGHIMP"
        );
        assert_eq!(
            la64_translator
                .basic_translate(
                    32,
                    &VariableValue::String("01011-hlw0010001000100010001000".to_owned())
                )
                .0,
            "DON'T CARE"
        );
    }
}

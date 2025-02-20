use crate::{
    parser::{take, take_n, Parser},
    AmlContext,
    AmlError,
};
use bit_field::BitField;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PkgLength {
    pub raw_length: u32,
    /// The distance from the end of the structure this `PkgLength` refers to, and the end of the
    /// stream.
    pub end_offset: u32,
}

impl PkgLength {
    pub fn from_raw_length(stream: &[u8], raw_length: u32) -> PkgLength {
        PkgLength { raw_length, end_offset: stream.len() as u32 - raw_length }
    }

    /// Returns `true` if the given stream is still within the structure this `PkgLength` refers
    /// to.
    pub fn still_parsing(&self, stream: &[u8]) -> bool {
        stream.len() as u32 > self.end_offset
    }
}

pub fn pkg_length<'a, 'c>() -> impl Parser<'a, 'c, PkgLength>
where
    'c: 'a,
{
    move |input: &'a [u8], context: &'c mut AmlContext| {
        let (new_input, context, raw_length) = raw_pkg_length().parse(input, context)?;

        /*
         * NOTE: we use the original input here, because `raw_length` includes the length of the
         * `PkgLength`.
         */
        Ok((new_input, context, PkgLength::from_raw_length(input, raw_length)))
    }
}

/// Parses a `PkgLength` and returns the *raw length*. If you want an instance of `PkgLength`, use
/// `pkg_length` instead.
pub fn raw_pkg_length<'a, 'c>() -> impl Parser<'a, 'c, u32>
where
    'c: 'a,
{
    /*
     * PkgLength := PkgLeadByte |
     * <PkgLeadByte ByteData> |
     * <PkgLeadByte ByteData ByteData> |
     * <PkgLeadByte ByteData ByteData ByteData>
     *
     * The length encoded by the PkgLength includes the number of bytes used to encode it.
     */
    move |input: &'a [u8], context: &'c mut AmlContext| {
        let (new_input, context, lead_byte) = take().parse(input, context)?;
        let byte_count = lead_byte.get_bits(6..8);

        if byte_count == 0 {
            let length = u32::from(lead_byte.get_bits(0..6));
            return Ok((new_input, context, length));
        }

        let (new_input, context, length): (&[u8], &mut AmlContext, u32) =
            match take_n(byte_count as u32).parse(new_input, context) {
                Ok((new_input, context, bytes)) => {
                    let initial_length = u32::from(lead_byte.get_bits(0..4));
                    (
                        new_input,
                        context,
                        bytes.iter().enumerate().fold(initial_length, |length, (i, &byte)| {
                            length + (u32::from(byte) << (4 + i * 8))
                        }),
                    )
                }

                /*
                 * The stream was too short. We return an error, making sure to return the
                 * *original* stream (that we haven't consumed any of).
                 */
                Err((_, context, _)) => return Err((input, context, AmlError::UnexpectedEndOfStream)),
            };

        Ok((new_input, context, length))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{test_utils::*, AmlError};

    fn test_correct_pkglength(stream: &[u8], expected_raw_length: u32, expected_leftover: &[u8]) {
        let mut context = AmlContext::new();
        check_ok!(
            pkg_length().parse(stream, &mut context),
            PkgLength::from_raw_length(stream, expected_raw_length),
            &expected_leftover
        );
    }

    #[test]
    fn test_raw_pkg_length() {
        let mut context = AmlContext::new();
        check_ok!(raw_pkg_length().parse(&[0b01000101, 0x14], &mut context), 325, &[]);
        check_ok!(raw_pkg_length().parse(&[0b01000111, 0x14, 0x46], &mut context), 327, &[0x46]);
        check_ok!(raw_pkg_length().parse(&[0b10000111, 0x14, 0x46], &mut context), 287047, &[]);
    }

    #[test]
    fn test_pkg_length() {
        let mut context = AmlContext::new();
        check_err!(pkg_length().parse(&[], &mut context), AmlError::UnexpectedEndOfStream, &[]);
        test_correct_pkglength(&[0x00], 0, &[]);
        test_correct_pkglength(&[0x05, 0xf5, 0x7f, 0x3e, 0x54, 0x03], 5, &[0xf5, 0x7f, 0x3e, 0x54, 0x03]);
        check_err!(
            pkg_length().parse(&[0b11000000, 0xff, 0x4f], &mut context),
            AmlError::UnexpectedEndOfStream,
            &[0b11000000, 0xff, 0x4f]
        );
    }

    #[test]
    #[should_panic]
    fn not_enough_stream() {
        /*
         * TODO: Ideally, this shouldn't panic the parser, but return a `UnexpectedEndOfStream`.
         */
        test_correct_pkglength(&[0x05, 0xf5], 5, &[0xf5]);
    }
}

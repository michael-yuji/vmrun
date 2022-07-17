pub mod os;
pub mod assertion;

use num_traits::Num;
use crate::spec::FormatError;

pub fn vec_exists<T, F>(vec: &[T], cond: F) -> bool 
  where F: Fn(&T) -> bool
{
    for i in vec.iter() {
        if cond(i) {
            return true;
        }
    }

    false
}

pub fn vec_sequence_map<E, A, B, F>(vec: &[A], map: F) -> Result<Vec<B>, E>
  where F: Fn(&A) -> Result<B, E>
{
    let mut v = Vec::new();

    for ins in vec.iter() {
        let b = map(ins)?;
        v.push(b);
    }

    Ok(v)
}

pub fn take_numeric<N: Num<FromStrRadixErr = std::num::ParseIntError>>(
    parse_radix: bool, input: &str) -> Result<(N, &str), FormatError>
{
    let mut char_view = input.char_indices();
    let mut index = 0;
    let mut parsed_radix = false;
    let mut radix = 10;

    loop {
        match char_view.next() {
            None => {
                break;
            },
            Some((_, value)) => {

                if parse_radix {
                    /* the first index is 0, so we proceed to check if need to 
                     * switch radix, base on the second index
                     */
                    if index == 0 && value == '0' {
                        parsed_radix = true;
                    }

                    /* the second index */
                    if index == 1 && parsed_radix {
                        /* 0x -> base 16 */
                        if let 'x' = value {
                            radix = 16;
                        } else if let 'b' = value {
                            /* 0b -> base 2 */
                            radix = 2;
                        } else if value.is_digit(8) {
                            /* 0[0-7]* -> base 8 */
                            radix = 8;
                        }

                        /* if we reached here, the char is either digit of
                         * [8, 9], or a none base 10 digit, we allow the
                         * condition to fallthrough, as if it is a digit,
                         * we should consume it as if it's in base 10,
                         * otherwise, the loop will be broken and we
                         * only cosumed the first 0, which is the right behaviour
                         */
                    }

                }

                if value.is_digit(radix) { 
                    index += 1;
                } else {
                    break
                }
            }
        }
    }

    let start: usize = 
        if radix == 16 || radix == 2 { 2 } else if radix == 8 { 1 } else { 0 };

    let value = N::from_str_radix(&input[start..index], radix)
        .map_err(FormatError::InvalidValue)?;

    Ok((value, &input[index..]))
}

pub fn parse_mem_in_kb(input: &String) -> Result<usize, FormatError>
{
    let (value, rest) = take_numeric::<usize>(true, input)?;
    let multipier: usize = (match rest {
        "K"|"KB"|"kb"|"Kb" => Ok(1),
        "M"|"MB"|"mb"|"Mb" => Ok(1024),
        "G"|"GB"|"gb"|"Gb" => Ok(1024 * 1024),
        "T"|"TB"|"tb"|"Tb" => Ok(1024 * 1024 * 1024),
        value => Err(FormatError::InvalidUnit(value.to_string()))
    })?;

    Ok(value * multipier)
}

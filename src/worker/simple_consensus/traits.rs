//! Consensus trait.
//!
//! This trait is used to check if a consensus has been reached within an iterator of results.

use std::hash::Hash;

use crate::AntsError;

pub trait Consensus<T: Eq + Hash> {
    /// Check if a consensus has been reached.
    fn consensus(self, min_ballot: usize) -> Result<T, AntsError>;
}

impl<I, T> Consensus<T> for I
where
    I: ExactSizeIterator<Item = T>,
    T: Eq + Hash,
{
    /// Check if a consensus has been reached.
    fn consensus(self, min_ballot: usize) -> Result<T, AntsError> {
        let mut counts = fxhash::FxHashMap::default();

        logger::debug!(
            "Minimum ballot {min_ballot} out of {total}.",
            min_ballot = min_ballot,
            total = self.len()
        );

        for item in self.into_iter() {
            // Count the number of times we have seen this item.
            let count = counts.entry(fxhash::hash(&item)).or_insert(0);
            *count += 1;

            if *count >= min_ballot {
                logger::info!("Consensus reached with {count} votes.", count = count);
                return Ok(item);
            }
        }

        let max_agreed = counts.into_values().max().unwrap_or(0);

        // We should never reach this point.
        assert!(
            max_agreed < min_ballot,
            "Unreachable: max_agreed: {}, min_ballot: {}",
            max_agreed,
            min_ballot
        );
        logger::debug!(
            "Consensus pending with {max_agreed} out of {min_ballot} votes.",
            max_agreed = max_agreed,
            min_ballot = min_ballot
        );
        Err(AntsError::ConsensusPending(max_agreed))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    macro_rules! expand_tests {
        ($(($name:ident, $input:expr, $min_ballot:expr, $expected:expr)),*$(,)?) => {
            $(
                #[test]
                fn $name() {
                    assert_eq!(
                        $input.into_iter().consensus($min_ballot).map_err(
                            |err| match err {
                                AntsError::ConsensusPending(agreed) => agreed,
                                _ => panic!("Unexpected error: {:?}", err),
                            }
                        ),
                        $expected
                    )
                }
            )*
        }
    }

    expand_tests!(
        (empty, vec![], 1, Err::<(), _>(0)),
        (single, vec![()], 1, Ok(())),
        (single_insufficient_ballot, vec![()], 2, Err(1)),
        (two_agreed, vec![1, 1], 2, Ok(1)),
        (two_disagreed, vec![1, 2], 2, Err(1)),
        (three_agreed, vec![1, 1, 2], 2, Ok(1)),
        (three_disagreed, vec![1, 2, 3], 2, Err(1)),
        (seven_agreed, vec![2, 2, 0, 0, 0, 1, 1], 3, Ok(0)),
        (seven_disagreed, vec![2, 2, 0, 0, 0, 1, 1], 4, Err(3)),
    );
}

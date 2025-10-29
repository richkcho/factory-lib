#[cfg(test)]
mod simple_tests {
    #[test]
    fn simple_rr_merge_test() {
        /**
         * The intial configuration is:
         * Belt 1: [ Stack(A, 1) x 5 , Stack(B, 1) x 5 ]
         * Belt 2: [ Stack(A, 1) x 5 , Stack(B, 1) x 5 ]
         * Run both belts into a merger with equal priority. (merge happens in round-robin fashion)
         * The expected output is:
         * Merged Belt: [ Stack(A, 1) x 10 , Stack(B, 1) x 10 ]
         */
    }
}
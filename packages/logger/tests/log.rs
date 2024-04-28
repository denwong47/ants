use logger;

macro_rules! expand_levels {
    ($($level:ident),+$(,)?) => {
        $(
            #[test]
            fn $level() {
                logger::$level!("Hello, world!");
                logger::$level!("Hello, {}!", "world");
            }
        )*
    };
}

expand_levels!(debug, info, warn, error,);

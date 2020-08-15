#[macro_export]
macro_rules! decl_tx {
	(
		$(
			fn $name:ident(
				$runtime:ident,
				$origin:ident
				$(, $arg_name:ident : $arg_value:ty)* $(,)?
			) {  $( $impl:tt )* }
		)*

	) => {
		$(
			fn $name<
				R: $crate::ModuleRuntime
			>(
				$runtime: &R,
				$origin: $crate::AccountId
				$(, $arg_name: $arg_value)*
			) -> $crate::DispatchResult {
				$( $impl )*
			}
		)*
	};
}

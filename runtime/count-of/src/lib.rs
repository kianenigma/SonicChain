use inflector::Inflector;
use quote::quote;

#[proc_macro_derive(CountOf)]
pub fn count_of(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
	let input = syn::parse_macro_input!(input as syn::ItemEnum);

	let enum_name = input.ident.clone();
	let trait_name = syn::Ident::new(
		&format!("{}Count", input.ident.to_string()),
		proc_macro2::Span::call_site(),
	);

	let functions = input
		.variants
		.iter()
		.map(|v| {
			let variant_name = v.ident.clone().to_string().to_snake_case();
			let variant_name = syn::Ident::new(
				&format!("{}_count", variant_name),
				proc_macro2::Span::call_site(),
			);
			quote! {
				fn #variant_name(&self) -> usize;
			}
		})
		.collect::<proc_macro2::TokenStream>();

	let function_implementations = input
		.variants
		.iter()
		.map(|v| {
			let variant_name = v.ident.clone();
			let fields = if v.fields.len() > 0 {
				let fields = v
					.fields
					.iter()
					.map(|_| quote! { _, })
					.collect::<proc_macro2::TokenStream>();
				quote! { (#fields) }
			} else {
				quote! {}
			};

			let variant_and_fields = quote! { #variant_name #fields };
			let variant_name_snake = v.ident.clone().to_string().to_snake_case();
			let variant_name_snake = syn::Ident::new(
				&format!("{}_count", variant_name_snake),
				proc_macro2::Span::call_site(),
			);

			quote! {
				fn #variant_name_snake(&self) -> usize {
					self.iter().filter(|x| std::matches!(x, #enum_name::#variant_and_fields)).count()
				}
			}
		})
		.collect::<proc_macro2::TokenStream>();

	let output: proc_macro2::TokenStream = quote! {
		pub trait #trait_name {
			#functions
		}

		impl #trait_name for Vec<#enum_name> {
			#function_implementations
		}
	};

	proc_macro::TokenStream::from(output)
}

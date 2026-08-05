mod book_number;

#[topcoat::router::path_param]
struct Collection(str);

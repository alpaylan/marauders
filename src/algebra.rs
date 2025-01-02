type Tag = String;
type Id = String;

pub(crate) enum Expr {
    Sum(Box<Expr>, Box<Expr>),
    Product(Box<Expr>, Box<Expr>),
    USum(Tag),
    UProduct(Tag),
    Id(Id),
}

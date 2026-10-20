//! Операции над ФС. Одиночные быстрые (mkdir, rename) выполняются задачей
//! `Lane::Interactive` без job; копирование, перенос и удаление — через движок (S5-04).

pub(crate) mod copy;
pub mod delete;
pub mod mkdir;
pub mod rename;
pub(crate) mod trash;

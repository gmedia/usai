create table invoices (id serial primary key, user_id int not null references users(id), amount numeric(12,2) not null);

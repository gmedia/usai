// The error is in a *test* file, which the application's own tsconfig
// excludes — so only checking `tsconfig.json` leaves it unseen, and a suite
// can be green over a test that does not compile.
const wrong: number = "not a number";
export default wrong;

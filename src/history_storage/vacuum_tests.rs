use super::*;

#[test]
fn bounded_vacuum_reclaims_the_full_page_budget() {
    let connection = rusqlite::Connection::open_in_memory().unwrap();
    connection
        .execute_batch(
            "PRAGMA auto_vacuum=INCREMENTAL;
             CREATE TABLE vacuum_fixture(payload BLOB NOT NULL);",
        )
        .unwrap();
    connection
        .execute("INSERT INTO vacuum_fixture VALUES(zeroblob(4194304))", [])
        .unwrap();
    connection
        .execute("DELETE FROM vacuum_fixture", [])
        .unwrap();
    let free = || {
        connection
            .query_row("PRAGMA freelist_count", [], |row| row.get::<_, u32>(0))
            .unwrap()
    };
    let before = free();
    assert!(before > VACUUM_PAGES * 2);
    maintain_sqlite(&connection).unwrap();
    assert_eq!(before - free(), VACUUM_PAGES);
    let remaining = free();
    incremental_vacuum(&connection, 0).unwrap();
    assert_eq!(free(), remaining);
    incremental_vacuum(&connection, 512).unwrap();
    assert_eq!(remaining - free(), 512);
    assert!(free() > 0);
    assert_eq!(
        connection
            .query_row("SELECT count(*) FROM vacuum_fixture", [], |row| {
                row.get::<_, u64>(0)
            })
            .unwrap(),
        0
    );
}

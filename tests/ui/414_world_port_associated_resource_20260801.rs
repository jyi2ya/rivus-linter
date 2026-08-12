// check-pass
// compile-flags: --test
#![feature(register_tool)]
#![register_tool(rivus)]
#![allow(non_snake_case)]
#![allow(rivus::rvs_untested_ok_fn)]

#[derive(Debug)]
enum TransportError {
    Unavailable,
    Closed,
}

trait ByteTransport {
    type World;
    type Connection;

    fn rvs_connect_MP(world: &mut Self::World) -> Result<Self::Connection, TransportError>;
    fn rvs_write_MP(
        world: &mut Self::World,
        connection: &mut Self::Connection,
        bytes: &[u8],
    ) -> Result<usize, TransportError>;
    fn rvs_shutdown_MP(world: &mut Self::World, connection: Self::Connection);
}

#[derive(Debug, Default)]
struct FakeWorld {
    written: Vec<u8>,
}

#[derive(Debug)]
struct FakeConnection {
    open: bool,
}

#[derive(Debug)]
struct FakeTransport;

impl ByteTransport for FakeTransport {
    type World = FakeWorld;
    type Connection = FakeConnection;

    fn rvs_connect_MP(_world: &mut Self::World) -> Result<Self::Connection, TransportError> {
        Ok(FakeConnection { open: true })
    }

    fn rvs_write_MP(
        world: &mut Self::World,
        connection: &mut Self::Connection,
        bytes: &[u8],
    ) -> Result<usize, TransportError> {
        if !connection.open {
            return Err(TransportError::Closed);
        }
        world.written.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn rvs_shutdown_MP(_world: &mut Self::World, mut connection: Self::Connection) {
        connection.open = false;
    }
}

fn rvs_send_MP<T: ByteTransport>(
    world: &mut T::World,
    bytes: &[u8],
) -> Result<(), TransportError> {
    let mut connection = T::rvs_connect_MP(world)?;
    let _ = T::rvs_write_MP(world, &mut connection, bytes)?;
    T::rvs_shutdown_MP(world, connection);
    Ok(())
}

#[test]
fn test_20260801_world_port_associated_resource() {
    let mut world = FakeWorld::default();
    rvs_send_MP::<FakeTransport>(&mut world, b"hello")
        .expect("never: fake transport accepts an open connection");
    assert_eq!(world.written, b"hello");
    let _ = TransportError::Unavailable;
}

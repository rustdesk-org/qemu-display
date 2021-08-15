use futures::stream::{self, StreamExt};
use std::convert::TryFrom;
use zbus::azync::Connection;
use zbus::fdo::ManagedObjects;
use zvariant::OwnedObjectPath;

use crate::{Audio, Chardev, Result, UsbRedir};

pub struct Display {
    conn: Connection,
    objects: ManagedObjects,
}

impl Display {
    pub async fn new(conn: &Connection) -> Result<Self> {
        let objects = zbus::fdo::AsyncObjectManagerProxy::builder(&conn)
            .destination("org.qemu")?
            .path("/org/qemu/Display1")?
            .build()
            .await?
            .get_managed_objects()
            .await?;
        // TODO: listen for changes ?
        Ok(Self {
            conn: conn.clone(),
            objects,
        })
    }

    pub async fn audio(&self) -> Result<Option<Audio>> {
        if !self
            .objects
            .contains_key(&OwnedObjectPath::try_from("/org/qemu/Display1/Audio").unwrap())
        {
            return Ok(None);
        }

        Ok(Some(Audio::new(&self.conn).await?))
    }

    pub async fn chardevs(&self) -> Vec<Chardev> {
        stream::iter(&self.objects)
            .filter_map(|(p, _ifaces)| async move {
                match p.strip_prefix("/org/qemu/Display1/Chardev_") {
                    Some(id) => Chardev::new(&self.conn, id).await.ok(),
                    _ => None,
                }
            })
            .collect()
            .await
    }

    pub async fn usbredir(&self) -> UsbRedir {
        let chardevs = stream::iter(self.chardevs().await)
            .filter_map(|c| async move {
                if c.proxy.name().await.ok() == Some("org.qemu.usbredir".to_string()) {
                    Some(c)
                } else {
                    None
                }
            })
            .collect()
            .await;

        let redir = UsbRedir::new(chardevs);
        redir
    }
}

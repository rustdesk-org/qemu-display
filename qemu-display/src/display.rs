use futures::stream::{self, StreamExt};
use std::{
    collections::HashMap,
    convert::{TryFrom, TryInto},
};
use zbus::{
    fdo,
    fdo::ManagedObjects,
    names::{BusName, OwnedUniqueName, UniqueName, WellKnownName},
    Connection,
};
use zvariant::OwnedObjectPath;

use crate::{Audio, Chardev, Clipboard, Error, Result, UsbRedir, VMProxy};

pub struct Display {
    conn: Connection,
    objects: ManagedObjects,
}

impl Display {
    pub async fn by_name(conn: &Connection) -> Result<HashMap<String, OwnedUniqueName>> {
        let mut hm = HashMap::new();
        let list = match fdo::DBusProxy::new(conn)
            .await?
            .list_queued_owners(WellKnownName::from_str_unchecked("org.qemu"))
            .await
        {
            Ok(list) => list,
            Err(zbus::fdo::Error::NameHasNoOwner(_)) => vec![],
            Err(e) => return Err(e.into()),
        };
        for dest in list.into_iter() {
            let name = VMProxy::builder(conn)
                .destination(UniqueName::from(&dest))?
                .build()
                .await?
                .name()
                .await?;
            hm.insert(name, dest);
        }
        Ok(hm)
    }

    pub async fn new<'d, D>(conn: &Connection, dest: Option<D>) -> Result<Self>
    where
        D: TryInto<BusName<'d>>,
        D::Error: Into<Error>,
    {
        let dest: BusName = if let Some(dest) = dest {
            dest.try_into().map_err(Into::into)?
        } else {
            "org.qemu".try_into().unwrap()
        };
        let objects = fdo::ObjectManagerProxy::builder(conn)
            .destination(dest)?
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

    pub async fn clipboard(&self) -> Result<Option<Clipboard>> {
        if !self
            .objects
            .contains_key(&OwnedObjectPath::try_from("/org/qemu/Display1/Clipboard").unwrap())
        {
            return Ok(None);
        }

        Ok(Some(Clipboard::new(&self.conn).await?))
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

        UsbRedir::new(chardevs)
    }
}

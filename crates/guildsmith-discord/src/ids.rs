//! Conversions between guildsmith's `Id` and twilight's typed ids.

use guildsmith_app::ports::{PortError, PortResult};
use guildsmith_core::Id;
use twilight_model::id::Id as TId;
use twilight_model::id::marker::{ChannelMarker, GuildMarker, MessageMarker, RoleMarker, UserMarker};

fn to<M>(id: Id) -> PortResult<TId<M>> {
    TId::new_checked(id.0).ok_or_else(|| PortError::NotFound("id 0 is not a Discord id".to_owned()))
}

pub(crate) fn guild(id: Id) -> PortResult<TId<GuildMarker>> {
    to(id)
}

pub(crate) fn channel(id: Id) -> PortResult<TId<ChannelMarker>> {
    to(id)
}

pub(crate) fn user(id: Id) -> PortResult<TId<UserMarker>> {
    to(id)
}

pub(crate) fn message(id: Id) -> PortResult<TId<MessageMarker>> {
    to(id)
}

pub(crate) fn role(id: Id) -> PortResult<TId<RoleMarker>> {
    to(id)
}

pub(crate) fn from<M>(id: TId<M>) -> Id {
    Id(id.get())
}

pub(crate) fn from_user(id: TId<UserMarker>) -> Id {
    Id(id.get())
}

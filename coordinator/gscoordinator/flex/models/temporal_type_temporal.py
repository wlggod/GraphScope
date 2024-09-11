from datetime import date, datetime  # noqa: F401

from typing import List, Dict  # noqa: F401

from gscoordinator.flex.models.base_model import Model
from gscoordinator.flex.models.date_type import DateType
from gscoordinator.flex.models.time_stamp_type import TimeStampType
from gscoordinator.flex import util

from gscoordinator.flex.models.date_type import DateType  # noqa: E501
from gscoordinator.flex.models.time_stamp_type import TimeStampType  # noqa: E501

class TemporalTypeTemporal(Model):
    """NOTE: This class is auto generated by OpenAPI Generator (https://openapi-generator.tech).

    Do not edit the class manually.
    """

    def __init__(self, timestamp=None, date32=None):  # noqa: E501
        """TemporalTypeTemporal - a model defined in OpenAPI

        :param timestamp: The timestamp of this TemporalTypeTemporal.  # noqa: E501
        :type timestamp: str
        :param date32: The date32 of this TemporalTypeTemporal.  # noqa: E501
        :type date32: str
        """
        self.openapi_types = {
            'timestamp': str,
            'date32': str
        }

        self.attribute_map = {
            'timestamp': 'timestamp',
            'date32': 'date32'
        }

        self._timestamp = timestamp
        self._date32 = date32

    @classmethod
    def from_dict(cls, dikt) -> 'TemporalTypeTemporal':
        """Returns the dict as a model

        :param dikt: A dict.
        :type: dict
        :return: The TemporalType_temporal of this TemporalTypeTemporal.  # noqa: E501
        :rtype: TemporalTypeTemporal
        """
        return util.deserialize_model(dikt, cls)

    @property
    def timestamp(self) -> str:
        """Gets the timestamp of this TemporalTypeTemporal.


        :return: The timestamp of this TemporalTypeTemporal.
        :rtype: str
        """
        return self._timestamp

    @timestamp.setter
    def timestamp(self, timestamp: str):
        """Sets the timestamp of this TemporalTypeTemporal.


        :param timestamp: The timestamp of this TemporalTypeTemporal.
        :type timestamp: str
        """
        if timestamp is None:
            raise ValueError("Invalid value for `timestamp`, must not be `None`")  # noqa: E501

        self._timestamp = timestamp

    @property
    def date32(self) -> str:
        """Gets the date32 of this TemporalTypeTemporal.


        :return: The date32 of this TemporalTypeTemporal.
        :rtype: str
        """
        return self._date32

    @date32.setter
    def date32(self, date32: str):
        """Sets the date32 of this TemporalTypeTemporal.


        :param date32: The date32 of this TemporalTypeTemporal.
        :type date32: str
        """
        if date32 is None:
            raise ValueError("Invalid value for `date32`, must not be `None`")  # noqa: E501

        self._date32 = date32